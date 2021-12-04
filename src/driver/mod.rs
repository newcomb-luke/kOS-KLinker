use crate::driver::errors::{LinkError, ProcessingError};
use crate::tables::{
    ContextHash, DataTable, Function, MasterSymbolEntry, NameTable, NameTableEntry, ObjectData,
    SymbolTable, TempInstr, TempOperand,
};
use crate::CLIConfig;
use errors::LinkResult;
use kerbalobjects::kofile::symbols::{SymBind, SymType};
use kerbalobjects::kofile::KOFile;
use kerbalobjects::ksmfile::sections::{ArgumentSection, CodeSection, DebugEntry, DebugRange};
use kerbalobjects::ksmfile::{Instr, KSMFile};
use kerbalobjects::KOSValue;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;
use std::panic;
use std::thread::{self, JoinHandle};

pub mod reader;
use reader::Reader;

use self::errors::{FileErrorContext, FuncErrorContext};

pub mod errors;

pub struct Driver {
    config: CLIConfig,
    thread_handles: Vec<JoinHandle<LinkResult<ObjectData>>>,
}

impl Driver {
    pub fn new(config: CLIConfig) -> Self {
        Driver {
            config,
            thread_handles: Vec::with_capacity(16),
        }
    }

    pub fn add(&mut self, path: &str) {
        let path_string = String::from(path);
        let handle = thread::spawn(move || {
            let (file_name, kofile) = Reader::read_file(path_string)?;
            Reader::process_file(file_name, kofile)
        });
        self.thread_handles.push(handle);
    }

    pub fn add_file(&mut self, file_name: String, kofile: KOFile) {
        let handle = thread::spawn(move || Reader::process_file(file_name, kofile));
        self.thread_handles.push(handle);
    }

    pub fn link(&mut self) -> LinkResult<KSMFile> {
        let mut object_data = Vec::with_capacity(self.thread_handles.len());

        for handle in self.thread_handles.drain(..) {
            let data = match handle.join() {
                Ok(obj_data) => obj_data?,
                Err(e) => panic::resume_unwind(e),
            };

            object_data.push(data);
        }

        let init_hash = {
            let mut hasher = DefaultHasher::new();

            hasher.write("_init".as_bytes());

            hasher.finish()
        };

        let entry_point_hash = {
            // If this should be linked as a shared object
            if self.config.shared {
                init_hash
            }
            // If not, then it is the entry point provided
            else {
                let mut hasher = DefaultHasher::new();
                hasher.write(self.config.entry_point.as_bytes());
                hasher.finish()
            }
        };

        let mut master_data_table = DataTable::new();
        let mut master_symbol_table = NameTable::<MasterSymbolEntry>::new();
        let mut master_function_vec = Vec::new();
        let mut init_function = None;
        let mut start_function = None;
        let mut master_function_name_table = NameTable::<NonZeroUsize>::new();
        let mut file_name_table = NameTable::<()>::new();
        let mut master_comment: Option<String> = None;

        let mut temporary_function_vec = Vec::new();

        let mut ksm_file = KSMFile::new();
        let arg_section = ksm_file.arg_section_mut();
        // We only have one single code section that contains all executable instructions
        let mut code_section = CodeSection::new(kerbalobjects::ksmfile::sections::CodeType::Main);

        // Maps data hashes to arg section indexes
        let mut data_hash_map = HashMap::<u64, usize>::new();
        // Maps function name hashes to absolute instruction indexes
        let mut func_hash_map = HashMap::<u64, usize>::new();
        // Keeps track of all of the functions that are referenced
        let mut func_ref_vec: Vec<u64> = Vec::new();
        // Variable to keep track of the current absolute index of each function
        let mut func_offset = 0;

        // Resolve all symbols
        for (object_data_index, data) in object_data.iter_mut().enumerate() {
            let mut hasher = DefaultHasher::new();
            hasher.write(data.input_file_name.as_bytes());
            let file_name_hash = ContextHash::FileNameHash(hasher.finish());
            let file_entry = NameTableEntry::from(data.input_file_name.to_owned(), ());
            let file_name_index = file_name_table.insert(file_entry);

            // Add all function names
            for mut func_entry in data.function_name_table.drain() {
                // Update the file name index
                func_entry.set_value(file_name_index);
                master_function_name_table.insert(func_entry);
            }

            // Set all function object data indexes
            for func in data.function_table.functions_mut() {
                func.set_object_data_index(object_data_index);
            }
            for func in data.local_function_table.functions_mut() {
                func.set_object_data_index(object_data_index);
            }

            // Resolve all symbols in this file
            Driver::resolve_symbols(
                &mut master_symbol_table,
                &mut master_data_table,
                &master_function_name_table,
                file_name_hash,
                data,
                &mut master_comment,
                entry_point_hash,
            )?;

            // Add all of the data in this file
            for value in data.data_table.entries() {
                master_data_table.add(value.clone());
            }
        }

        // At this point all of the symbols will have been resolved. Now we should check if there
        // are any external symbols left (bad!)
        for symbol_entry in master_symbol_table.entries() {
            if symbol_entry.value().internal().sym_bind() == SymBind::Extern {
                let name = symbol_entry.name().to_owned();
                return Err(LinkError::UnresolvedExternalSymbolError(name));
            }
        }

        // Loop through all global functions
        for data in object_data.iter_mut() {
            for func in data.function_table.drain() {
                if func.name_hash() == init_hash {
                    init_function = Some(func);
                } else if func.name_hash() == entry_point_hash {
                    start_function = Some(func);
                } else {
                    temporary_function_vec.push(func);
                }
            }
        }

        // Add _init and _start to the top if they exist
        if let Some(init_func) = &init_function {
            temporary_function_vec.insert(0, init_func.clone());
            func_ref_vec.push(init_func.name_hash());
        } else {
            // If we are a shared library, that is required
            if self.config.shared {
                return Err(LinkError::MissingInitFunctionError);
            }
        }

        if let Some(start_func) = &start_function {
            // _init should go before _start
            if init_function.is_some() {
                temporary_function_vec.insert(1, start_func.clone());
            } else {
                temporary_function_vec.insert(0, start_func.clone());
            }

            func_ref_vec.push(start_func.name_hash());
        } else {
            // If we are not a shared library, that is required
            if !self.config.shared {
                return Err(LinkError::MissingEntryPointError(
                    self.config.entry_point.to_owned(),
                ));
            }
        }

        // The two "root" functions for optimization are _init and _start
        if let Some(init_func) = &init_function {
            Driver::add_func_refs_optimize(
                init_func.name_hash(),
                true,
                &mut func_ref_vec,
                init_func.object_data_index(),
                &mut object_data,
                &master_symbol_table,
                &temporary_function_vec,
            );
        }

        if let Some(start_func) = &start_function {
            Driver::add_func_refs_optimize(
                start_func.name_hash(),
                true,
                &mut func_ref_vec,
                start_func.object_data_index(),
                &mut object_data,
                &master_symbol_table,
                &temporary_function_vec,
            );
        }

        // Now add all of the functions that are referenced
        for data in object_data.iter_mut() {
            for func in temporary_function_vec.drain(..) {
                // Check the reference list
                if func_ref_vec.contains(&func.name_hash()) {
                    master_function_vec.push(func);
                }
            }

            for func in data.local_function_table.drain() {
                if data.local_function_ref_vec.contains(&func.name_hash()) {
                    master_function_vec.push(func);
                }
            }
        }

        // Add in the comment if it exists
        if let Some(comment) = master_comment {
            let value = KOSValue::String(comment);
            arg_section.add(value);
        }

        // Loop through each function and find it's offset
        for func in master_function_vec.iter() {
            func_offset = Driver::calc_func_offset(
                func,
                object_data.get_mut(func.object_data_index()).unwrap(),
                &mut func_hash_map,
                func_offset,
            );
        }

        // Now add the functions to the binary
        for mut func in master_function_vec {
            let object_data_index = func.object_data_index();
            Driver::add_func_to_code_section(
                &mut func,
                arg_section,
                &mut code_section,
                &master_symbol_table,
                &master_data_table,
                &master_function_name_table,
                &func_hash_map,
                &mut data_hash_map,
                &object_data.get(object_data_index).unwrap(),
            )?;
        }

        let init_section =
            CodeSection::new(kerbalobjects::ksmfile::sections::CodeType::Initialization);
        let func_section = CodeSection::new(kerbalobjects::ksmfile::sections::CodeType::Function);

        ksm_file.add_code_section(func_section);
        ksm_file.add_code_section(init_section);
        ksm_file.add_code_section(code_section);

        let mut debug_entry = DebugEntry::new(1);
        debug_entry.add(DebugRange::new(2, 4));

        ksm_file.debug_section_mut().add(debug_entry);

        Ok(ksm_file)
    }

    fn add_func_to_code_section(
        func: &mut Function,
        arg_section: &mut ArgumentSection,
        code_section: &mut CodeSection,
        master_symbol_table: &NameTable<MasterSymbolEntry>,
        master_data_table: &DataTable,
        master_function_name_table: &NameTable<NonZeroUsize>,
        func_hash_map: &HashMap<u64, usize>,
        data_hash_map: &mut HashMap<u64, usize>,
        object_data: &ObjectData,
    ) -> LinkResult<()> {
        let mut instr_index = 0;

        for instr in func.drain() {
            let concrete = Driver::concrete_instr(
                instr,
                arg_section,
                master_symbol_table,
                master_data_table,
                master_function_name_table,
                func_hash_map,
                data_hash_map,
                object_data,
                func.name_hash(),
                instr_index,
            )?;
            instr_index += 1;

            code_section.add(concrete);
        }

        Ok(())
    }

    fn func_hash_from_op(
        op: &TempOperand,
        master_symbol_table: &NameTable<MasterSymbolEntry>,
        local_symbol_table: &SymbolTable,
    ) -> Option<(bool, u64)> {
        // If it is a symbol reference
        if let TempOperand::SymNameHash(hash) = op {
            // Local symbols have higher priority
            if let Some(sym) = local_symbol_table.get_by_hash(*hash) {
                // If it is a function
                if sym.internal().sym_type() == SymType::Func {
                    // The boolean represents if it was a global symbol
                    Some((false, *hash))
                } else {
                    None
                }
            } else if let Some(sym) = master_symbol_table.get_by_hash(*hash) {
                if sym.value().internal().sym_type() == SymType::Func {
                    Some((true, *hash))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    }

    fn add_func_ref_from_op(
        op: &TempOperand,
        func_ref_vec: &mut Vec<u64>,
        parent_object_data_index: usize,
        object_data: &mut Vec<ObjectData>,
        master_symbol_table: &NameTable<MasterSymbolEntry>,
        temporary_function_vec: &Vec<Function>,
    ) {
        if let Some((is_global, hash)) = Driver::func_hash_from_op(
            op,
            master_symbol_table,
            &object_data
                .get(parent_object_data_index)
                .unwrap()
                .local_symbol_table,
        ) {
            let referenced_func_opt = {
                if is_global {
                    if !func_ref_vec.contains(&hash) {
                        func_ref_vec.push(hash);

                        let referenced_func = temporary_function_vec
                            .iter()
                            .find(|func| func.name_hash() == hash)
                            .unwrap();

                        let referenced_func_name_hash = referenced_func.name_hash();
                        let func_object_data_index = referenced_func.object_data_index();

                        Some((referenced_func_name_hash, func_object_data_index))
                    } else {
                        None
                    }
                } else {
                    let parent_object_data = object_data.get_mut(parent_object_data_index).unwrap();

                    if !parent_object_data.local_function_ref_vec.contains(&hash) {
                        parent_object_data.local_function_ref_vec.push(hash);

                        let referenced_func = object_data
                            .get(parent_object_data_index)
                            .unwrap()
                            .local_function_table
                            .get_by_hash(hash)
                            .unwrap();

                        let referenced_func_name_hash = referenced_func.name_hash();
                        let func_object_data_index = referenced_func.object_data_index();

                        Some((referenced_func_name_hash, func_object_data_index))
                    } else {
                        None
                    }
                }
            };

            if let Some((referenced_name_hash, referenced_object_data_index)) = referenced_func_opt
            {
                // Recurse.
                Driver::add_func_refs_optimize(
                    referenced_name_hash,
                    is_global,
                    func_ref_vec,
                    referenced_object_data_index,
                    object_data,
                    master_symbol_table,
                    temporary_function_vec,
                );
            }
        }
    }

    fn add_func_refs_optimize(
        func_name_hash: u64,
        func_is_global: bool,
        func_ref_vec: &mut Vec<u64>,
        object_data_index: usize,
        object_data: &mut Vec<ObjectData>,
        master_symbol_table: &NameTable<MasterSymbolEntry>,
        temporary_function_vec: &Vec<Function>,
    ) {
        let mut op_vec = Vec::with_capacity(16);
        let parent_func = if func_is_global {
            temporary_function_vec
                .iter()
                .find(|func| func.name_hash() == func_name_hash)
                .unwrap()
        } else {
            object_data
                .get(object_data_index)
                .unwrap()
                .local_function_table
                .get_by_hash(func_name_hash)
                .unwrap()
        };

        for instr in parent_func.instructions() {
            match instr {
                TempInstr::ZeroOp(_) => {}
                TempInstr::OneOp(_, op1) => {
                    op_vec.push(*op1);
                }
                TempInstr::TwoOp(_, op1, op2) => {
                    op_vec.push(*op1);
                    op_vec.push(*op2);
                }
            }
        }

        for op in op_vec {
            Driver::add_func_ref_from_op(
                &op,
                func_ref_vec,
                object_data_index,
                object_data,
                master_symbol_table,
                temporary_function_vec,
            );
        }
    }

    fn calc_func_offset(
        func: &Function,
        object_data: &mut ObjectData,
        func_hash_map: &mut HashMap<u64, usize>,
        current_offset: usize,
    ) -> usize {
        let size = func.instruction_count();

        if func.is_global() {
            func_hash_map.insert(func.name_hash(), current_offset);
        } else {
            object_data
                .local_function_hash_map
                .insert(func.name_hash(), current_offset);
        }

        current_offset + size
    }

    fn concrete_instr(
        temp: TempInstr,
        arg_section: &mut ArgumentSection,
        master_symbol_table: &NameTable<MasterSymbolEntry>,
        master_data_table: &DataTable,
        master_function_name_table: &NameTable<NonZeroUsize>,
        func_hash_map: &HashMap<u64, usize>,
        data_hash_map: &mut HashMap<u64, usize>,
        object_data: &ObjectData,
        func_name_hash: u64,
        instr_index: usize,
    ) -> LinkResult<Instr> {
        let func_name = match object_data
            .local_function_name_table
            .get_by_hash(func_name_hash)
        {
            Some(func) => func.name(),
            None => master_function_name_table
                .get_by_hash(func_name_hash)
                .unwrap()
                .name(),
        };

        match temp {
            TempInstr::ZeroOp(opcode) => Ok(Instr::ZeroOp(opcode)),
            TempInstr::OneOp(opcode, op1) => {
                let op1_idx = Driver::tempop_to_concrete(
                    op1,
                    arg_section,
                    master_symbol_table,
                    master_data_table,
                    func_hash_map,
                    data_hash_map,
                    object_data,
                    func_name,
                    instr_index,
                )?;

                Ok(Instr::OneOp(opcode, op1_idx))
            }
            TempInstr::TwoOp(opcode, op1, op2) => {
                let op1_idx = Driver::tempop_to_concrete(
                    op1,
                    arg_section,
                    master_symbol_table,
                    master_data_table,
                    func_hash_map,
                    data_hash_map,
                    object_data,
                    func_name,
                    instr_index,
                )?;
                let op2_idx = Driver::tempop_to_concrete(
                    op2,
                    arg_section,
                    master_symbol_table,
                    master_data_table,
                    func_hash_map,
                    data_hash_map,
                    object_data,
                    func_name,
                    instr_index,
                )?;

                Ok(Instr::TwoOp(opcode, op1_idx, op2_idx))
            }
        }
    }

    fn tempop_to_concrete(
        op: TempOperand,
        arg_section: &mut ArgumentSection,
        master_symbol_table: &NameTable<MasterSymbolEntry>,
        master_data_table: &DataTable,
        func_hash_map: &HashMap<u64, usize>,
        data_hash_map: &mut HashMap<u64, usize>,
        object_data: &ObjectData,
        func_name: &String,
        instr_index: usize,
    ) -> LinkResult<usize> {
        match op {
            TempOperand::DataHash(hash) => match data_hash_map.get(&hash) {
                Some(index) => Ok(*index),
                None => {
                    // We do this nonsense so that only referenced data is included in the final binary
                    let value = master_data_table.get_by_hash(hash).unwrap();
                    let index = arg_section.add(value.clone());
                    data_hash_map.insert(hash, index);

                    Ok(index)
                }
            },
            TempOperand::SymNameHash(hash) => {
                let sym = match object_data.local_symbol_table.get_by_hash(hash) {
                    Some(local_sym) => local_sym.internal(),
                    None => match master_symbol_table.get_by_hash(hash) {
                        Some(entry) => entry.value().internal(),
                        None => {
                            return Err(LinkError::InvalidSymbolRefError(
                                func_name.to_owned(),
                                instr_index,
                                hash,
                            ));
                        }
                    },
                };

                match sym.sym_type() {
                    SymType::Func => {
                        let func_loc = if sym.sym_bind() == SymBind::Global {
                            func_hash_map.get(&hash).unwrap()
                        } else {
                            object_data.local_function_hash_map.get(&hash).unwrap()
                        };

                        // Construct a new String that contains the destination label
                        let value = KOSValue::String(format!("@{:0>4}", *func_loc));

                        let mut hasher = DefaultHasher::new();
                        value.hash(&mut hasher);
                        let data_hash = hasher.finish();

                        match data_hash_map.get(&data_hash) {
                            Some(index) => Ok(*index),
                            None => {
                                let index = arg_section.add(value.clone());
                                data_hash_map.insert(data_hash, index);

                                Ok(index)
                            }
                        }
                    }
                    SymType::NoType => {
                        // SAFETY: As usual, we add 1 so it is safe
                        let index = unsafe { NonZeroUsize::new_unchecked(sym.value_idx() + 1) };

                        let data_hash = master_data_table.hash_at(index).unwrap();

                        match data_hash_map.get(&data_hash) {
                            Some(index) => Ok(*index),
                            None => {
                                let value = master_data_table.get_at(index).unwrap();
                                let index = arg_section.add(value.clone());
                                data_hash_map.insert(*data_hash, index);

                                Ok(index)
                            }
                        }
                    }
                    _ => unreachable!("Symbol type is not of NoType or Func"),
                }
            }
        }
    }

    fn resolve_symbols(
        master_symbol_table: &mut NameTable<MasterSymbolEntry>,
        master_data_table: &mut DataTable,
        master_function_name_table: &NameTable<NonZeroUsize>,
        file_name_hash: ContextHash,
        object_data: &mut ObjectData,
        comment: &mut Option<String>,
        entry_point_hash: u64,
    ) -> LinkResult<()> {
        for mut symbol in object_data.symbol_table.drain() {
            let name_entry = object_data
                .symbol_name_table
                .get_by_hash(symbol.name_hash())
                .unwrap();

            // If it is not a local symbol
            if symbol.internal().sym_bind() != SymBind::Local {
                // If it is a function symbol
                if symbol.internal().sym_type() == SymType::Func {
                    // Set the context to be correct
                    symbol.set_context(file_name_hash);

                    // If it is the entry point, try to set the comment
                    if entry_point_hash == symbol.name_hash() {
                        *comment = object_data.comment.clone();
                    }
                }

                match master_symbol_table.get_by_hash(symbol.name_hash()) {
                    Some(other_symbol) => {
                        // If the found symbol is external
                        if other_symbol.value().internal().sym_bind() == SymBind::Extern {
                            // If this new symbol is _not_ external
                            if symbol.internal().sym_bind() != SymBind::Extern {
                                let new_data_idx;

                                if symbol.internal().sym_type() != SymType::Func {
                                    let data_index = unsafe {
                                        NonZeroUsize::new_unchecked(
                                            symbol.internal().value_idx() + 1,
                                        )
                                    };
                                    let data = object_data.data_table.get_at(data_index).unwrap();

                                    let (_, non_zero_idx) = master_data_table.add(data.clone());

                                    new_data_idx = non_zero_idx.get() - 1;
                                } else {
                                    // If this is a function, set the data index to 0, it won't be needed
                                    new_data_idx = 0;
                                }

                                symbol.internal_mut().set_value_idx(new_data_idx);
                                let new_symbol = symbol.internal().clone();

                                let new_symbol_entry =
                                    MasterSymbolEntry::new(new_symbol, symbol.context());

                                // Replace it
                                master_symbol_table
                                    .replace_by_hash(symbol.name_hash(), new_symbol_entry)
                                    .map_err(|_| {
                                        LinkError::InternalError(String::from(
                                            "Symbol name hash invalid.",
                                        ))
                                    })?;
                            }
                            // If it was external, don't do anything
                        }
                        // If it isn't external
                        else {
                            // Check if we are not external
                            if symbol.internal().sym_bind() != SymBind::Extern {
                                // Duplicate symbol!

                                let file_error_context = FileErrorContext {
                                    input_file_name: object_data.input_file_name.to_owned(),
                                    source_file_name: object_data.source_file_name.to_owned(),
                                };

                                let mut func_error_context = FuncErrorContext {
                                    file_context: file_error_context.clone(),
                                    func_name: String::new(),
                                };

                                let mut original_func_name = None;

                                if let ContextHash::FuncNameHash(func_name_hash) =
                                    other_symbol.value().context()
                                {
                                    let original_function_name_entry = master_function_name_table
                                        .get_by_hash(func_name_hash)
                                        .unwrap();
                                    let original_function_name =
                                        original_function_name_entry.name();

                                    original_func_name = Some(original_function_name.to_owned());
                                }

                                return Err(match original_func_name {
                                    Some(name) => {
                                        func_error_context.func_name = name;

                                        LinkError::FuncContextError(
                                            func_error_context,
                                            ProcessingError::DuplicateSymbolError(
                                                name_entry.name().to_owned(),
                                                object_data.source_file_name.to_owned(),
                                            ),
                                        )
                                    }
                                    None => LinkError::FileContextError(
                                        file_error_context,
                                        ProcessingError::DuplicateSymbolError(
                                            name_entry.name().to_owned(),
                                            object_data.source_file_name.to_owned(),
                                        ),
                                    ),
                                });
                            }
                            // If we are external, then just continue
                        }
                    }
                    None => {
                        let new_data_idx;

                        if symbol.internal().sym_type() != SymType::Func {
                            let data_index = unsafe {
                                NonZeroUsize::new_unchecked(symbol.internal().value_idx() + 1)
                            };

                            let data = object_data.data_table.get_at(data_index).unwrap();

                            let (_, non_zero_idx) = master_data_table.add(data.clone());

                            new_data_idx = non_zero_idx.get() - 1;
                        } else {
                            // If this is a function, set the data index to 0, it won't be needed
                            new_data_idx = 0;
                        }

                        symbol.internal_mut().set_value_idx(new_data_idx);
                        let new_symbol = symbol.internal().clone();

                        let new_symbol_entry = MasterSymbolEntry::new(new_symbol, symbol.context());
                        let new_name_entry =
                            NameTableEntry::from(name_entry.name().to_owned(), new_symbol_entry);

                        master_symbol_table.raw_insert(symbol.name_hash(), new_name_entry);
                    }
                }
            }
        }

        Ok(())
    }
}
