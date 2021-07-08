use crate::driver::errors::{LinkError, ProcessingError};
use crate::tables::{
    ContextHash, DataTable, Function, FunctionTable, MasterSymbolEntry, NameTable, ObjectData,
    SymbolEntry, SymbolTable, TempInstr, TempOperand,
};
use crate::CLIConfig;
use errors::LinkResult;
use kerbalobjects::kofile::sections::{ReldSection, SectionIndex};
use kerbalobjects::kofile::symbols::{KOSymbol, SymBind, SymType};
use kerbalobjects::kofile::KOFile;
use kerbalobjects::ksmfile::KSMFile;
use kerbalobjects::{FromBytes, KOSValue};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::ffi::OsString;
use std::hash::Hasher;
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
        let handle = thread::spawn(move || Driver::process_file(path_string));
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

        let entry_point_hash = {
            let mut hasher = DefaultHasher::new();

            // If this should be linked as a shared object
            if self.config.shared {
                // Then the "entry point" is "_init"
                hasher.write("_init".as_bytes());
            }
            // If not, then it is the entry point provided
            else {
                hasher.write(self.config.entry_point.as_bytes());
            }

            hasher.finish()
        };

        let mut master_data_table = DataTable::new();
        let mut master_symbol_table = NameTable::<MasterSymbolEntry>::new();
        let mut master_function_vec = Vec::new();
        let mut master_function_name_table = NameTable::<NonZeroUsize>::new();
        let mut file_name_table = NameTable::<()>::new();
        let mut master_comment: Option<String> = None;
        let mut master_comment_kosvalue = None;

        let mut data_size = 3; // Offset for %A in KSM file
        let mut index_size = 0;

        // Resolve all symbols
        for data in object_data.iter_mut() {
            let mut hasher = DefaultHasher::new();
            hasher.write(data.input_file_name.as_bytes());
            let file_name_hash = ContextHash::FileNameHash(hasher.finish());
            file_name_table.insert(data.input_file_name.clone(), ());

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

        // Factor in the comment if it exists
        match master_comment {
            Some(comment) => {
                let value = KOSValue::String(comment);
                data_size += value.size_bytes();

                master_comment_kosvalue = Some(value);
            }
            None => {}
        }

        // Calculate the size of instruction operands
        // This is required to find out how large the functions will be
        data_size += master_data_table.size_bytes();

        index_size = Driver::size_to_hold(data_size);

        // Maximum number of bytes supported is 4
        if index_size > 4 {
            return Err(LinkError::DataIndexOverflowError);
        }

        println!("Master symbol table: {:#?}", master_symbol_table);

        println!("Master data table: {:#?}", master_data_table);

        for data in object_data.iter() {
            for func in data.function_table.functions() {
                for instr in func.instructions() {}
            }
        }

        unimplemented!();
    }

    fn size_to_hold(number: usize) -> usize {
        if number < 0x000000FF {
            // 1 byte
            1
        } else if number < 0x0000FFFF {
            // 2 bytes
            2
        } else if number < 0x00FFFFFF {
            // 3 bytes
            3
        } else if number < 0xFFFFFFFF {
            // 4 bytes
            4
        } else {
            // Arbitrary
            5
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
                            let data_index = unsafe {
                                NonZeroUsize::new_unchecked(symbol.internal().value_idx() + 1)
                            };
                            let data = object_data.data_table.get_at(data_index).unwrap();

                            let (_, new_data_idx) = master_data_table.add(data.clone());

                            let new_symbol = KOSymbol::new(
                                0,
                                new_data_idx.get() - 1,
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
                    master_symbol_table.raw_insert(
                        symbol.name_hash(),
                        name_entry.clone().into(),
                        symbol.into(),
                    );
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

    fn process_file(path: String) -> LinkResult<ObjectData> {
        let mut hasher = DefaultHasher::new();

        let (file_name, kofile) = Driver::read_file(path)?;
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

            function_name_table.insert(name.to_owned(), unsafe { NonZeroUsize::new_unchecked(1) }); // 1 is a placeholder here because there is no file name table to reference

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
                    new_data_entry.1.get(),
                    symbol.size(),
                    symbol.sym_bind(),
                    symbol.sym_type(),
                    symbol.sh_idx(),
                );

                let symbol_entry = SymbolEntry::new(name_hash, new_symbol, file_name_hash);

                let table_index = symbol_table.add(symbol_entry);
                symbol_name_table.insert(name.to_owned(), table_index);
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
                            new_data_entry.1.get(),
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
                    symbol_name_table.insert(name.to_owned(), table_index);

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
