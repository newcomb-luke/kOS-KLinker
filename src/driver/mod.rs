use crate::driver::errors::{LinkError, ProcessingError};
use crate::tables::{
    ContextHash, DataTable, Function, FunctionTable, MasterSymbolEntry, NameTable, NameTableEntry,
    ObjectData, SymbolEntry, SymbolTable, TempInstr, TempOperand,
};
use crate::CLIConfig;
use errors::LinkResult;
use kerbalobjects::kofile::sections::{ReldSection, SectionIndex};
use kerbalobjects::kofile::symbols::{KOSymbol, SymBind, SymType};
use kerbalobjects::kofile::KOFile;
use kerbalobjects::ksmfile::sections::{ArgumentSection, CodeSection, DebugEntry, DebugRange};
use kerbalobjects::ksmfile::{Instr, KSMFile};
use kerbalobjects::{FromBytes, KOSValue};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::ffi::OsString;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::num::NonZeroUsize;
use std::panic;
use std::path::Path;
use std::thread::{self, JoinHandle};

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
            let (file_name, kofile) = Driver::read_file(path_string)?;
            Driver::process_file(file_name, kofile)
        });
        self.thread_handles.push(handle);
    }

    pub fn add_file(&mut self, file_name: String, kofile: KOFile) {
        let handle = thread::spawn(move || Driver::process_file(file_name, kofile));
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

        let mut ksm_file = KSMFile::new();
        let arg_section = ksm_file.arg_section_mut();
        // We only have one single code section that contains all executable instructions
        let mut code_section = CodeSection::new(kerbalobjects::ksmfile::sections::CodeType::Main);

        // Maps data hashes to arg section indexes
        let mut data_hash_map = HashMap::<u64, usize>::new();
        // Maps function name hashes to absolute instruction indexes
        let mut func_hash_map = HashMap::<u64, usize>::new();
        // Keeps track of all of the functions that are referenced
        let mut func_ref_vec = Vec::new();

        // Resolve all symbols
        for data in object_data.iter_mut() {
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

            // Resolve all symbols in this file
            Driver::resolve_symbols(
                &mut master_symbol_table,
                &mut master_data_table,
                &master_function_name_table,
                &file_name_table,
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

        // This really sucks. But it is the only way to know if a function is actually used
        // TODO: Fix this somehow?
        for data in object_data.iter() {
            for func in data.function_table.functions() {
                for instr in func.instructions() {
                    match instr {
                        TempInstr::ZeroOp(_) => {}
                        TempInstr::OneOp(_, op1) => {
                            // If it is a symbol reference
                            if let TempOperand::SymNameHash(hash) = op1 {
                                // If it exists (it will at this point)
                                if let Some(sym) = master_symbol_table.get_by_hash(*hash) {
                                    // If it is a function
                                    if sym.value().internal().sym_type() == SymType::Func {
                                        // Then that function was referenced
                                        func_ref_vec.push(*hash);
                                    }
                                }
                            }
                        }
                        TempInstr::TwoOp(_, op1, op2) => {
                            if let TempOperand::SymNameHash(hash) = op1 {
                                if let Some(sym) = master_symbol_table.get_by_hash(*hash) {
                                    if sym.value().internal().sym_type() == SymType::Func {
                                        func_ref_vec.push(*hash);
                                    }
                                }
                            }
                            if let TempOperand::SymNameHash(hash) = op2 {
                                if let Some(sym) = master_symbol_table.get_by_hash(*hash) {
                                    if sym.value().internal().sym_type() == SymType::Func {
                                        func_ref_vec.push(*hash);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Now add all of the functions that are referenced
        for mut data in object_data {
            for func in data.function_table.drain() {
                if func.name_hash() == init_hash {
                    init_function = Some(func);
                } else if func.name_hash() == entry_point_hash {
                    start_function = Some(func);
                } else {
                    // If it isn't special, check the reference list
                    if func_ref_vec.contains(&func.name_hash()) {
                        master_function_vec.push(func);
                    }
                }
            }
        }

        // Add in the comment if it exists
        if let Some(comment) = master_comment {
            let value = KOSValue::String(comment);
            arg_section.add(value);
        }

        // Add the init function (if it exists)
        match &init_function {
            Some(func) => {
                let size = func.instruction_count();

                func_hash_map.insert(func.name_hash(), size);
            }
            None => {
                // If we are a shared library, that is required
                if self.config.shared {
                    return Err(LinkError::MissingInitFunctionError);
                }
            }
        }
        // Add the entry point (if it exists)
        match &start_function {
            Some(func) => {
                let size = func.instruction_count();

                func_hash_map.insert(func.name_hash(), size);
            }
            None => {
                // If we are not a shared library, that is required
                if !self.config.shared {
                    return Err(LinkError::MissingEntryPointError(
                        self.config.entry_point.to_owned(),
                    ));
                }
            }
        }
        // Loop through each function and find it's offset
        for func in master_function_vec.iter() {
            let size = func.instruction_count();

            func_hash_map.insert(func.name_hash(), size);
        }

        // Add init first
        if let Some(mut func) = init_function {
            for instr in func.drain() {
                let concrete = Driver::concrete_instr(
                    instr,
                    arg_section,
                    &master_symbol_table,
                    &master_data_table,
                    &func_hash_map,
                    &mut data_hash_map,
                );

                code_section.add(concrete);
            }
        }

        // If we are trying to create an executable, add in the entry point code as well
        if !self.config.shared {
            if let Some(mut func) = start_function {
                for instr in func.drain() {
                    let concrete = Driver::concrete_instr(
                        instr,
                        arg_section,
                        &master_symbol_table,
                        &master_data_table,
                        &func_hash_map,
                        &mut data_hash_map,
                    );

                    code_section.add(concrete);
                }
            }
        }

        // Now add the rest of the functions
        for mut func in master_function_vec {
            for instr in func.drain() {
                let concrete = Driver::concrete_instr(
                    instr,
                    arg_section,
                    &master_symbol_table,
                    &master_data_table,
                    &func_hash_map,
                    &mut data_hash_map,
                );

                code_section.add(concrete);
            }
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

    fn concrete_instr(
        temp: TempInstr,
        arg_section: &mut ArgumentSection,
        master_symbol_table: &NameTable<MasterSymbolEntry>,
        master_data_table: &DataTable,
        func_hash_map: &HashMap<u64, usize>,
        data_hash_map: &mut HashMap<u64, usize>,
    ) -> Instr {
        match temp {
            TempInstr::ZeroOp(opcode) => Instr::ZeroOp(opcode),
            TempInstr::OneOp(opcode, op1) => {
                let op1_idx = Driver::tempop_to_concrete(
                    op1,
                    arg_section,
                    master_symbol_table,
                    master_data_table,
                    func_hash_map,
                    data_hash_map,
                );

                Instr::OneOp(opcode, op1_idx)
            }
            TempInstr::TwoOp(opcode, op1, op2) => {
                let op1_idx = Driver::tempop_to_concrete(
                    op1,
                    arg_section,
                    master_symbol_table,
                    master_data_table,
                    func_hash_map,
                    data_hash_map,
                );
                let op2_idx = Driver::tempop_to_concrete(
                    op2,
                    arg_section,
                    master_symbol_table,
                    master_data_table,
                    func_hash_map,
                    data_hash_map,
                );

                Instr::TwoOp(opcode, op1_idx, op2_idx)
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
    ) -> usize {
        match op {
            TempOperand::DataHash(hash) => match data_hash_map.get(&hash) {
                Some(index) => *index,
                None => {
                    // We do this nonsense so that only referenced data is included in the final binary
                    let value = master_data_table.get_by_hash(hash).unwrap();
                    let index = arg_section.add(value.clone());
                    data_hash_map.insert(hash, index);

                    index
                }
            },
            TempOperand::SymNameHash(hash) => {
                let sym = master_symbol_table
                    .get_by_hash(hash)
                    .unwrap()
                    .value()
                    .internal();

                match sym.sym_type() {
                    SymType::Func => {
                        let func_loc = func_hash_map.get(&hash).unwrap();

                        // Construct a new Int32 value that contains the location
                        let value = KOSValue::Int32(*func_loc as i32);

                        let mut hasher = DefaultHasher::new();
                        value.hash(&mut hasher);
                        let data_hash = hasher.finish();

                        match data_hash_map.get(&data_hash) {
                            Some(index) => *index,
                            None => {
                                let index = arg_section.add(value.clone());
                                data_hash_map.insert(data_hash, index);

                                index
                            }
                        }
                    }
                    SymType::NoType => {
                        // SAFETY: As usual, we add 1 so it is safe
                        let index = unsafe { NonZeroUsize::new_unchecked(sym.value_idx() + 1) };

                        let data_hash = master_data_table.hash_at(index).unwrap();

                        match data_hash_map.get(&data_hash) {
                            Some(index) => *index,
                            None => {
                                let value = master_data_table.get_at(index).unwrap();
                                let index = arg_section.add(value.clone());
                                data_hash_map.insert(*data_hash, index);

                                index
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
        file_name_table: &NameTable<()>,
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
                                    NonZeroUsize::new_unchecked(symbol.internal().value_idx() + 1)
                                };
                                let data = object_data.data_table.get_at(data_index).unwrap();

                                let (_, non_zero_idx) = master_data_table.add(data.clone());

                                new_data_idx = non_zero_idx.get() - 1;
                            } else {
                                // If this is a function, set the data index to 0, it won't be needed
                                new_data_idx = 0;
                            }

                            let new_symbol = KOSymbol::new(
                                0,
                                new_data_idx,
                                symbol.internal().size(),
                                symbol.internal().sym_bind(),
                                symbol.internal().sym_type(),
                                symbol.internal().sh_idx(),
                            );

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

                            let original_file_name = match other_symbol.value().context() {
                                ContextHash::FuncNameHash(func_name_hash) => {
                                    let original_function_name_entry = master_function_name_table
                                        .get_by_hash(func_name_hash)
                                        .unwrap();
                                    let original_function_name =
                                        original_function_name_entry.name();
                                    let original_file_name = file_name_table
                                        .get_at(*original_function_name_entry.value())
                                        .unwrap()
                                        .name();

                                    original_func_name = Some(original_function_name.to_owned());

                                    original_file_name.to_owned()
                                }
                                ContextHash::FileNameHash(file_name_hash) => file_name_table
                                    .get_by_hash(file_name_hash)
                                    .unwrap()
                                    .name()
                                    .to_owned(),
                            };

                            return Err(match original_func_name {
                                Some(name) => {
                                    func_error_context.func_name = name;

                                    LinkError::FuncContextError(
                                        func_error_context,
                                        ProcessingError::DuplicateSymbolError(
                                            name_entry.name().to_owned(),
                                            original_file_name,
                                        ),
                                    )
                                }
                                None => LinkError::FileContextError(
                                    file_error_context,
                                    ProcessingError::DuplicateSymbolError(
                                        name_entry.name().to_owned(),
                                        original_file_name,
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

                    let new_symbol = KOSymbol::new(
                        0,
                        new_data_idx,
                        symbol.internal().size(),
                        symbol.internal().sym_bind(),
                        symbol.internal().sym_type(),
                        symbol.internal().sh_idx(),
                    );

                    let new_symbol_entry = MasterSymbolEntry::new(new_symbol, symbol.context());
                    let new_name_entry =
                        NameTableEntry::from(name_entry.name().to_owned(), new_symbol_entry);

                    master_symbol_table.raw_insert(symbol.name_hash(), new_name_entry);
                }
            }
        }

        Ok(())
    }

    fn read_file(path: String) -> LinkResult<(String, KOFile)> {
        let copied_path = String::clone(&path);
        let path_obj = Path::new(&path);

        let file_name_os = path_obj
            .file_name()
            .ok_or(LinkError::InvalidPathError(copied_path))?;
        let file_name = file_name_os
            .to_owned()
            .into_string()
            .map_err(|_| LinkError::StringConversionError)?;

        let mut buffer = Vec::with_capacity(2048);
        let mut file = std::fs::File::open(&path)
            .map_err(|e| LinkError::IOError(OsString::from(file_name_os), e.kind()))?;
        file.read_to_end(&mut buffer).unwrap();
        let mut buffer_iter = buffer.iter().peekable();

        Ok((
            file_name,
            KOFile::from_bytes(&mut buffer_iter, false)
                .map_err(|error| LinkError::FileReadError(OsString::from(file_name_os), error))?,
        ))
    }

    fn process_file(file_name: String, kofile: KOFile) -> LinkResult<ObjectData> {
        let mut hasher = DefaultHasher::new();

        hasher.write(file_name.as_bytes());
        let file_name_hash = ContextHash::FileNameHash(hasher.finish());

        let comment = match kofile.str_tab_by_name(".comment") {
            Some(section) => match section.get(0) {
                Some(name) => Some(name.to_owned()),
                None => None,
            },
            None => None,
        };

        let symtab = kofile
            .sym_tab_by_name(".symtab")
            .ok_or(LinkError::MissingSectionError(
                file_name.to_owned(),
                String::from(".symtab"),
            ))?;
        let symstrtab =
            kofile
                .str_tab_by_name(".symstrtab")
                .ok_or(LinkError::MissingSectionError(
                    file_name.to_owned(),
                    String::from(".symstrtab"),
                ))?;
        let data_section =
            kofile
                .data_section_by_name(".data")
                .ok_or(LinkError::MissingSectionError(
                    file_name.to_owned(),
                    String::from(".data"),
                ))?;
        let reld_section_opt = kofile.reld_section_by_name(".reld");

        let mut reld_map = HashMap::<usize, HashMap<usize, (Option<usize>, Option<usize>)>>::new();

        let mut symbol_table = SymbolTable::new();
        let mut function_table = FunctionTable::new();
        let mut data_table = DataTable::new();
        let mut symbol_name_table = NameTable::<NonZeroUsize>::new();
        let mut function_name_table = NameTable::<NonZeroUsize>::new();

        match reld_section_opt {
            Some(reld_section) => {
                Driver::process_relocations(&reld_section, &mut reld_map);
            }
            None => {}
        }

        let mut file_symbol_opt = None;

        // Find the file symbol
        for symbol in symtab.symbols() {
            if symbol.sym_type() == SymType::File {
                file_symbol_opt = Some(symbol);
                break;
            }
        }

        let file_symbol =
            file_symbol_opt.ok_or(LinkError::MissingFileSymbolError(file_name.to_owned()))?;
        let source_file_name = symstrtab
            .get(file_symbol.name_idx())
            .ok_or(LinkError::MissingFileSymbolNameError(file_name.to_owned()))?
            .to_owned();

        let file_error_context = FileErrorContext {
            input_file_name: file_name.to_owned(),
            source_file_name: source_file_name.to_owned(),
        };

        let mut data_index_map = HashMap::<usize, (u64, NonZeroUsize)>::new();

        for (i, value) in data_section.data().enumerate() {
            let new_entry = data_table.add(value.clone());

            data_index_map.insert(i, new_entry);
        }

        let mut referenced_symbol_map = HashMap::<usize, NonZeroUsize>::with_capacity(64);

        // Loop through each function section
        for func_section in kofile.func_sections() {
            let name = kofile
                .sh_name_from_index(func_section.section_index())
                .ok_or(LinkError::MissingFunctionNameError(
                    file_name.to_owned(),
                    source_file_name.to_owned(),
                    func_section.section_index(),
                ))?;

            let func_error_context = FuncErrorContext {
                file_context: file_error_context.clone(),
                func_name: name.to_owned(),
            };

            function_name_table.insert(NameTableEntry::from(name.to_owned(), unsafe {
                NonZeroUsize::new_unchecked(1)
            })); // 1 is a placeholder here because there is no file name table to reference

            hasher = DefaultHasher::new();
            hasher.write(name.as_bytes());

            let hash_value = hasher.finish();

            let func_name_hash = ContextHash::FuncNameHash(hash_value);

            let mut function_entry = Function::new(hash_value);

            let func_reld = reld_map.get(&func_section.section_index());

            for (i, instr) in func_section.instructions().enumerate() {
                let temp_instr = match instr {
                    kerbalobjects::kofile::instructions::Instr::ZeroOp(opcode) => {
                        TempInstr::ZeroOp(*opcode)
                    }
                    kerbalobjects::kofile::instructions::Instr::OneOp(opcode, op1) => {
                        match func_reld.map(|reld| reld.get(&i)).flatten() {
                            Some(data) => TempInstr::OneOp(
                                *opcode,
                                Driver::tempop_from(
                                    &symtab,
                                    &symstrtab,
                                    &func_error_context,
                                    &data_index_map,
                                    &mut referenced_symbol_map,
                                    &mut symbol_table,
                                    &mut symbol_name_table,
                                    func_name_hash,
                                    i,
                                    data.0,
                                    *op1,
                                )?,
                            ),
                            None => TempInstr::OneOp(
                                *opcode,
                                Driver::data_tempop_from(
                                    &func_error_context,
                                    &data_index_map,
                                    i,
                                    *op1,
                                )?,
                            ),
                        }
                    }
                    kerbalobjects::kofile::instructions::Instr::TwoOp(opcode, op1, op2) => {
                        match func_reld.map(|reld| reld.get(&i)).flatten() {
                            Some(data) => TempInstr::TwoOp(
                                *opcode,
                                Driver::tempop_from(
                                    &symtab,
                                    &symstrtab,
                                    &func_error_context,
                                    &data_index_map,
                                    &mut referenced_symbol_map,
                                    &mut symbol_table,
                                    &mut symbol_name_table,
                                    func_name_hash,
                                    i,
                                    data.0,
                                    *op1,
                                )?,
                                Driver::tempop_from(
                                    &symtab,
                                    &symstrtab,
                                    &func_error_context,
                                    &data_index_map,
                                    &mut referenced_symbol_map,
                                    &mut symbol_table,
                                    &mut symbol_name_table,
                                    func_name_hash,
                                    i,
                                    data.1,
                                    *op2,
                                )?,
                            ),
                            None => TempInstr::TwoOp(
                                *opcode,
                                Driver::data_tempop_from(
                                    &func_error_context,
                                    &data_index_map,
                                    i,
                                    *op1,
                                )?,
                                Driver::data_tempop_from(
                                    &func_error_context,
                                    &data_index_map,
                                    i,
                                    *op2,
                                )?,
                            ),
                        }
                    }
                };

                function_entry.add(temp_instr);
            }

            function_table.add(function_entry);
        }

        // Add all non-referenced global symbols
        for (i, symbol) in symtab.symbols().enumerate() {
            if !referenced_symbol_map.contains_key(&i)
                && symbol.sym_bind() == SymBind::Global
                && symbol.sym_type() != SymType::File
            {
                let name = symstrtab
                    .get(symbol.name_idx())
                    .ok_or(LinkError::FileContextError(
                        file_error_context.clone(),
                        ProcessingError::MissingSymbolNameError(i, symbol.name_idx()),
                    ))?;
                hasher = DefaultHasher::new();
                hasher.write(name.as_bytes());
                let name_hash = hasher.finish();

                let new_data_entry =
                    data_index_map
                        .get(&symbol.value_idx())
                        .ok_or(LinkError::FileContextError(
                            file_error_context.clone(),
                            ProcessingError::InvalidSymbolDataIndexError(
                                name.to_owned(),
                                symbol.value_idx(),
                            ),
                        ))?;

                let new_symbol = KOSymbol::new(
                    symbol.name_idx(),
                    new_data_entry.1.get() - 1,
                    symbol.size(),
                    symbol.sym_bind(),
                    symbol.sym_type(),
                    symbol.sh_idx(),
                );

                let symbol_entry = SymbolEntry::new(name_hash, new_symbol, file_name_hash);

                let table_index = symbol_table.add(symbol_entry);
                symbol_name_table.insert(NameTableEntry::from(name.to_owned(), table_index));
            }
        }

        Ok(ObjectData {
            input_file_name: file_name,
            source_file_name,
            comment,
            symbol_name_table,
            function_name_table,
            function_table,
            symbol_table,
            data_table,
        })
    }

    fn tempop_from(
        symtab: &kerbalobjects::kofile::sections::SymbolTable,
        symstrtab: &kerbalobjects::kofile::sections::StringTable,
        func_error_context: &FuncErrorContext,
        data_index_map: &HashMap<usize, (u64, NonZeroUsize)>,
        referenced_symbol_map: &mut HashMap<usize, NonZeroUsize>,
        symbol_table: &mut SymbolTable,
        symbol_name_table: &mut NameTable<NonZeroUsize>,
        func_name_hash: ContextHash,
        instr_index: usize,
        reld_data: Option<usize>,
        operand: usize,
    ) -> LinkResult<TempOperand> {
        Ok(match reld_data {
            Some(sym_idx) => {
                // If this symbol has not been previously referenced
                if !referenced_symbol_map.contains_key(&sym_idx) {
                    let mut symbol = symtab
                        .get(sym_idx)
                        .ok_or(LinkError::FuncContextError(
                            func_error_context.clone(),
                            ProcessingError::InvalidSymbolIndexError(instr_index, sym_idx),
                        ))?
                        .clone();

                    let name =
                        symstrtab
                            .get(symbol.name_idx())
                            .ok_or(LinkError::FuncContextError(
                                func_error_context.clone(),
                                ProcessingError::MissingSymbolNameError(sym_idx, symbol.name_idx()),
                            ))?;

                    if symbol.sym_type() == SymType::NoType && symbol.sym_bind() != SymBind::Extern
                    {
                        let new_data_entry = data_index_map.get(&symbol.value_idx()).ok_or(
                            LinkError::FuncContextError(
                                func_error_context.clone(),
                                ProcessingError::InvalidSymbolDataIndexError(
                                    name.to_owned(),
                                    symbol.value_idx(),
                                ),
                            ),
                        )?;

                        symbol = KOSymbol::new(
                            symbol.name_idx(),
                            new_data_entry.1.get() - 1,
                            symbol.size(),
                            symbol.sym_bind(),
                            symbol.sym_type(),
                            symbol.sh_idx(),
                        );
                    }
                    let mut hasher = DefaultHasher::new();

                    hasher.write(name.as_bytes());
                    let name_hash = hasher.finish();

                    let symbol_entry = SymbolEntry::new(name_hash, symbol, func_name_hash);

                    let table_index = symbol_table.add(symbol_entry);
                    symbol_name_table.insert(NameTableEntry::from(name.to_owned(), table_index));

                    referenced_symbol_map.insert(sym_idx, table_index);

                    TempOperand::SymNameHash(name_hash)
                }
                // If it has
                else {
                    let name_hash = *symbol_name_table
                        .get_hash_at(*referenced_symbol_map.get(&sym_idx).unwrap())
                        .unwrap();
                    TempOperand::SymNameHash(name_hash)
                }
            }
            None => Driver::data_tempop_from(
                &func_error_context,
                &data_index_map,
                instr_index,
                operand,
            )?,
        })
    }

    fn data_tempop_from(
        func_error_context: &FuncErrorContext,
        data_index_map: &HashMap<usize, (u64, NonZeroUsize)>,
        instr_index: usize,
        operand: usize,
    ) -> LinkResult<TempOperand> {
        let data_result = *data_index_map
            .get(&operand)
            .ok_or(LinkError::FuncContextError(
                func_error_context.clone(),
                ProcessingError::InvalidDataIndexError(instr_index, operand),
            ))?;
        Ok(TempOperand::DataHash(data_result.0))
    }

    fn process_relocations(
        reld_section: &ReldSection,
        reld_map: &mut HashMap<usize, HashMap<usize, (Option<usize>, Option<usize>)>>,
    ) {
        for entry in reld_section.entries() {
            match reld_map.get_mut(&entry.section_index()) {
                Some(func_map) => match func_map.get_mut(&entry.instr_index()) {
                    Some(data) => match entry.operand_index() {
                        0 => data.0 = Some(entry.symbol_index()),
                        1 => data.1 = Some(entry.symbol_index()),
                        _ => unreachable!(),
                    },
                    None => {
                        let mut data = (None, None);

                        match entry.operand_index() {
                            0 => data.0 = Some(entry.symbol_index()),
                            1 => data.1 = Some(entry.symbol_index()),
                            _ => unreachable!(),
                        }

                        func_map.insert(entry.instr_index(), data);
                    }
                },
                None => {
                    let mut func_map = HashMap::new();

                    let mut data = (None, None);

                    match entry.operand_index() {
                        0 => data.0 = Some(entry.symbol_index()),
                        1 => data.1 = Some(entry.symbol_index()),
                        _ => unreachable!(),
                    }

                    func_map.insert(entry.instr_index(), data);

                    reld_map.insert(entry.section_index(), func_map);
                }
            }
        }
    }
}
