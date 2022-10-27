use std::collections::hash_map::Entry;
use std::path::PathBuf;
use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    ffi::OsString,
    hash::Hasher,
    io::Read,
    num::NonZeroUsize,
};

use kerbalobjects::{
    kofile::{
        sections::{ReldSection, SectionIndex},
        symbols::{SymBind, SymType},
        KOFile,
    },
    FromBytes,
};

use crate::tables::{
    ContextHash, DataTable, Function, FunctionTable, NameTable, NameTableEntry, ObjectData,
    SymbolEntry, SymbolTable, TempInstr, TempOperand,
};

use super::errors::{FileErrorContext, FuncErrorContext, LinkError, LinkResult, ProcessingError};

pub struct Reader {}

impl Reader {
    pub fn read_file(path: impl Into<PathBuf>) -> LinkResult<(String, KOFile)> {
        let path = path.into();

        let file_name_os = path
            .file_name()
            .ok_or_else(|| LinkError::InvalidPathError(path.to_str().unwrap().to_string()))?;
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

    pub fn process_file(file_name: String, kofile: KOFile) -> LinkResult<ObjectData> {
        let mut hasher = DefaultHasher::new();

        hasher.write(file_name.as_bytes());
        let file_name_hash = ContextHash::FileNameHash(hasher.finish());

        let comment = kofile
            .str_tab_by_name(".comment")
            .and_then(|section| section.get(1).cloned());

        let symtab = kofile.sym_tab_by_name(".symtab").ok_or_else(|| {
            LinkError::MissingSectionError(file_name.to_owned(), String::from(".symtab"))
        })?;
        let symstrtab = kofile.str_tab_by_name(".symstrtab").ok_or_else(|| {
            LinkError::MissingSectionError(file_name.to_owned(), String::from(".symstrtab"))
        })?;
        let data_section = kofile.data_section_by_name(".data").ok_or_else(|| {
            LinkError::MissingSectionError(file_name.to_owned(), String::from(".data"))
        })?;
        let reld_section_opt = kofile.reld_section_by_name(".reld");

        let mut reld_map = HashMap::<usize, HashMap<usize, (Option<usize>, Option<usize>)>>::new();

        let mut symbol_table = SymbolTable::new();
        let mut function_table = FunctionTable::new();
        let mut data_table = DataTable::new();
        let mut symbol_name_table = NameTable::<NonZeroUsize>::new();
        let mut function_name_table = NameTable::<NonZeroUsize>::new();

        let mut local_symbol_table = SymbolTable::new();
        let mut local_function_table = FunctionTable::new();
        let local_function_hash_map = HashMap::new();
        let mut local_function_name_table = NameTable::new();
        let local_function_ref_vec = Vec::new();

        if let Some(reld_section) = reld_section_opt {
            Reader::process_relocations(reld_section, &mut reld_map);
        }

        let mut file_symbol_opt = None;

        // Find the file symbol
        for symbol in symtab.symbols() {
            if symbol.sym_type() == SymType::File {
                file_symbol_opt = Some(symbol);
                break;
            }
        }

        let file_symbol = file_symbol_opt
            .ok_or_else(|| LinkError::MissingFileSymbolError(file_name.to_owned()))?;
        let source_file_name = symstrtab
            .get(file_symbol.name_idx())
            .ok_or_else(|| LinkError::MissingFileSymbolNameError(file_name.to_owned()))?
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
                .ok_or_else(|| {
                    LinkError::MissingFunctionNameError(
                        file_name.to_owned(),
                        source_file_name.to_owned(),
                        func_section.section_index(),
                    )
                })?;

            let func_error_context = FuncErrorContext {
                file_context: file_error_context.clone(),
                func_name: name.to_owned(),
            };

            let func_name_idx = symstrtab.find(name).ok_or_else(|| {
                LinkError::FuncContextError(
                    func_error_context.to_owned(),
                    ProcessingError::FuncMissingSymbolError,
                )
            })?;

            let func_symbol = symtab.find_has_name(func_name_idx).ok_or_else(|| {
                LinkError::FuncContextError(
                    func_error_context.to_owned(),
                    ProcessingError::FuncMissingSymbolError,
                )
            })?;

            if func_symbol.sym_type() != SymType::Func {
                return Err(LinkError::FuncContextError(
                    func_error_context.to_owned(),
                    ProcessingError::FuncSymbolInvalidTypeError,
                ));
            }

            let func_name_table_entry =
                NameTableEntry::from(name.to_owned(), unsafe { NonZeroUsize::new_unchecked(1) }); // 1 is a placeholder because there is no file name table to reference

            hasher = DefaultHasher::new();
            hasher.write(name.as_bytes());

            let hash_value = hasher.finish();

            let func_name_hash = ContextHash::FuncNameHash(hash_value);

            let mut function_entry =
                Function::new(hash_value, func_symbol.sym_bind() == SymBind::Global);

            let func_reld = reld_map.get(&func_section.section_index());

            for (i, instr) in func_section.instructions().enumerate() {
                let temp_instr = match instr {
                    kerbalobjects::kofile::instructions::Instr::ZeroOp(opcode) => {
                        TempInstr::ZeroOp(*opcode)
                    }
                    kerbalobjects::kofile::instructions::Instr::OneOp(opcode, op1) => {
                        match func_reld.and_then(|reld| reld.get(&i)) {
                            Some(data) => TempInstr::OneOp(
                                *opcode,
                                Reader::tempop_from(
                                    symtab,
                                    symstrtab,
                                    &func_error_context,
                                    &data_index_map,
                                    &mut referenced_symbol_map,
                                    &mut symbol_table,
                                    &mut symbol_name_table,
                                    &mut local_symbol_table,
                                    func_name_hash,
                                    i,
                                    data.0,
                                    *op1,
                                )?,
                            ),
                            None => TempInstr::OneOp(
                                *opcode,
                                Reader::data_tempop_from(
                                    &func_error_context,
                                    &data_index_map,
                                    i,
                                    *op1,
                                )?,
                            ),
                        }
                    }
                    kerbalobjects::kofile::instructions::Instr::TwoOp(opcode, op1, op2) => {
                        match func_reld.and_then(|reld| reld.get(&i)) {
                            Some(data) => TempInstr::TwoOp(
                                *opcode,
                                Reader::tempop_from(
                                    symtab,
                                    symstrtab,
                                    &func_error_context,
                                    &data_index_map,
                                    &mut referenced_symbol_map,
                                    &mut symbol_table,
                                    &mut symbol_name_table,
                                    &mut local_symbol_table,
                                    func_name_hash,
                                    i,
                                    data.0,
                                    *op1,
                                )?,
                                Reader::tempop_from(
                                    symtab,
                                    symstrtab,
                                    &func_error_context,
                                    &data_index_map,
                                    &mut referenced_symbol_map,
                                    &mut symbol_table,
                                    &mut symbol_name_table,
                                    &mut local_symbol_table,
                                    func_name_hash,
                                    i,
                                    data.1,
                                    *op2,
                                )?,
                            ),
                            None => TempInstr::TwoOp(
                                *opcode,
                                Reader::data_tempop_from(
                                    &func_error_context,
                                    &data_index_map,
                                    i,
                                    *op1,
                                )?,
                                Reader::data_tempop_from(
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

            if func_symbol.sym_bind() == SymBind::Global {
                function_name_table.insert(func_name_table_entry);
                function_table.add(function_entry);
            } else {
                local_function_name_table.insert(func_name_table_entry);
                local_function_table.add(function_entry);
            }
        }

        // Add all non-referenced global symbols
        for (i, symbol) in symtab.symbols().enumerate() {
            if !referenced_symbol_map.contains_key(&i)
                && symbol.sym_bind() == SymBind::Global
                && symbol.sym_type() != SymType::File
            {
                let name = symstrtab.get(symbol.name_idx()).ok_or_else(|| {
                    LinkError::FileContextError(
                        file_error_context.clone(),
                        ProcessingError::MissingSymbolNameError(i, symbol.name_idx()),
                    )
                })?;
                hasher = DefaultHasher::new();
                hasher.write(name.as_bytes());
                let name_hash = hasher.finish();

                let new_data_entry = data_index_map.get(&symbol.value_idx()).ok_or_else(|| {
                    LinkError::FileContextError(
                        file_error_context.clone(),
                        ProcessingError::InvalidSymbolDataIndexError(
                            name.to_owned(),
                            symbol.value_idx(),
                        ),
                    )
                })?;

                let mut new_symbol = *symbol;
                new_symbol.set_value_idx(new_data_entry.1.get() - 1);

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
            local_function_table,
            local_symbol_table,
            local_function_hash_map,
            local_function_name_table,
            local_function_ref_vec,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn tempop_from(
        symtab: &kerbalobjects::kofile::sections::SymbolTable,
        symstrtab: &kerbalobjects::kofile::sections::StringTable,
        func_error_context: &FuncErrorContext,
        data_index_map: &HashMap<usize, (u64, NonZeroUsize)>,
        referenced_symbol_map: &mut HashMap<usize, NonZeroUsize>,
        symbol_table: &mut SymbolTable,
        symbol_name_table: &mut NameTable<NonZeroUsize>,
        local_symbol_table: &mut SymbolTable,
        func_name_hash: ContextHash,
        instr_index: usize,
        reld_data: Option<usize>,
        operand: usize,
    ) -> LinkResult<TempOperand> {
        Ok(match reld_data {
            Some(sym_idx) => {
                // If this symbol has not been previously referenced
                match referenced_symbol_map.entry(sym_idx) {
                    Entry::Occupied(e) => {
                        let name_hash = *symbol_name_table.get_hash_at(*e.get()).unwrap();
                        TempOperand::SymNameHash(name_hash)
                    }
                    Entry::Vacant(e) => {
                        let mut symbol = *symtab.get(sym_idx).ok_or_else(|| {
                            LinkError::FuncContextError(
                                func_error_context.clone(),
                                ProcessingError::InvalidSymbolIndexError(instr_index, sym_idx),
                            )
                        })?;

                        let name = symstrtab.get(symbol.name_idx()).ok_or_else(|| {
                            LinkError::FuncContextError(
                                func_error_context.clone(),
                                ProcessingError::MissingSymbolNameError(sym_idx, symbol.name_idx()),
                            )
                        })?;

                        if symbol.sym_type() == SymType::NoType
                            && symbol.sym_bind() != SymBind::Extern
                        {
                            let new_data_entry =
                                data_index_map.get(&symbol.value_idx()).ok_or_else(|| {
                                    LinkError::FuncContextError(
                                        func_error_context.clone(),
                                        ProcessingError::InvalidSymbolDataIndexError(
                                            name.to_owned(),
                                            symbol.value_idx(),
                                        ),
                                    )
                                })?;

                            symbol.set_value_idx(new_data_entry.1.get() - 1);
                        }
                        let mut hasher = DefaultHasher::new();

                        hasher.write(name.as_bytes());
                        let name_hash = hasher.finish();

                        let symbol_entry = SymbolEntry::new(name_hash, symbol, func_name_hash);

                        if symbol.sym_bind() != SymBind::Local {
                            let table_index = symbol_table.add(symbol_entry);
                            symbol_name_table
                                .insert(NameTableEntry::from(name.to_owned(), table_index));

                            e.insert(table_index);
                        } else {
                            local_symbol_table.add(symbol_entry);
                        }

                        TempOperand::SymNameHash(name_hash)
                    }
                }
            }
            None => {
                Reader::data_tempop_from(func_error_context, data_index_map, instr_index, operand)?
            }
        })
    }

    fn data_tempop_from(
        func_error_context: &FuncErrorContext,
        data_index_map: &HashMap<usize, (u64, NonZeroUsize)>,
        instr_index: usize,
        operand: usize,
    ) -> LinkResult<TempOperand> {
        let data_result = *data_index_map.get(&operand).ok_or_else(|| {
            LinkError::FuncContextError(
                func_error_context.clone(),
                ProcessingError::InvalidDataIndexError(instr_index, operand),
            )
        })?;
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
