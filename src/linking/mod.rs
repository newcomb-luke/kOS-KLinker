use crate::{KOFile, KOSValue};
use std::{collections::HashMap, error::Error, todo};

use crate::ksm::{
    ArgumentSection, CodeSection, DebugEntry, DebugSection, Instr, KSMFile, SectionType,
};

use kerbalobjects::{RelInstruction, Symbol, SymbolInfo, SymbolType};

mod errors;
use errors::{LinkError, LinkResult};

#[derive(Debug)]
pub enum Operand {
    Value(KOSValue),
    FuncRef(String),
}

#[derive(Debug)]
pub struct SymInstr {
    opcode: u8,
    operands: Vec<Operand>,
}

impl SymInstr {
    pub fn new(opcode: u8, operands: Vec<Operand>) -> SymInstr {
        SymInstr { opcode, operands }
    }
}

pub struct Function {
    id: String,
    file: String,
    instr: Vec<SymInstr>,
}

impl Function {
    pub fn new(id: &str, file: &str, instr: Vec<SymInstr>) -> Function {
        Function {
            id: id.to_owned(),
            file: file.to_owned(),
            instr,
        }
    }
}

pub struct Linker {}

impl Linker {
    pub fn link(object_files: Vec<KOFile>, debug: bool, shared: bool) -> LinkResult<KSMFile> {
        let mut functions: Vec<Function> = Vec::new();
        let mut init_code = Vec::new();
        let mut main_code = Vec::new();
        let mut main_code_file = String::new();
        let mut comment = String::new();
        let ksm_file;
        let mut argument_section = ArgumentSection::new();
        let mut code_sections = Vec::new();
        let mut debug_section;
        let _total_file_len = 0;

        for object_file in object_files {
            let file_comment = Linker::get_comment(&object_file)?;
            let file_name = Linker::get_file_name(&object_file)?;
            let mut section_index = 4;

            for code_section in object_file.get_code_sections() {
                let func_sym = Linker::get_func_sym(&object_file, &file_name, section_index)?;
                let func_name = func_sym.name().to_owned();
                let func_info = func_sym.get_info();

                if func_info == SymbolInfo::GLOBAL {
                    if func_name == "_start" {
                        // If we already had a main section, then that is a duplicate symbol error
                        if !main_code_file.is_empty() {
                            return Err(LinkError::DuplicateSymbolError(
                                file_name,
                                "_start".into(),
                                main_code_file,
                            )
                            .into());
                        } else {
                            // Store the main code's name
                            main_code_file = file_name.to_owned();
                            // Store the instructions into the vector
                            main_code = Linker::rel_to_sym(
                                &object_file,
                                &file_name,
                                code_section.get_instructions(),
                            )?;
                            // Store this file's comment
                            comment = file_comment.to_owned();
                        }
                    } else if code_section.name() == ".init" {
                        let mut sym_instrs = Linker::rel_to_sym(
                            &object_file,
                            &file_name,
                            code_section.get_instructions(),
                        )?;

                        init_code.append(&mut sym_instrs);
                    } else {
                        let func_name = code_section.name().to_owned();

                        let sym_instrs = Linker::rel_to_sym(
                            &object_file,
                            &file_name,
                            code_section.get_instructions(),
                        )?;

                        let func_option = functions.iter().find(|f| f.id == func_name);

                        if func_option.is_some() {
                            let other_func_file = func_option.unwrap().file.to_owned();

                            return Err(LinkError::DuplicateSymbolError(
                                file_name,
                                func_name,
                                other_func_file,
                            )
                            .into());
                        }

                        let func = Function::new(&func_name, &file_name, sym_instrs);

                        functions.push(func);
                    }
                }

                section_index += 1;
            }
        }

        if debug {
            println!("Main code:");

            for instr in main_code.iter() {
                println!("\t{:?}", instr);
            }
    
            println!("Init code:");
    
            for instr in init_code.iter() {
                println!("\t{:?}", instr);
            }
    
            println!("Functions:");
    
            for func in functions.iter() {
                println!("\tFunc: {}", func.id);
                println!("\tFile: {}", func.file);
                for instr in func.instr.iter() {
                    println!("\t\t{:?}", instr);
                }
            }
        }

        // Add the comment if there is one
        if !comment.is_empty() {
            argument_section.add(KOSValue::STRING(comment))?;
        }

        if !shared {
            // This will build an executable

            let lbrt = SymInstr::new(
                0xf0,
                vec![Operand::Value(KOSValue::STRING(String::from("@0001")))],
            );
            main_code.insert(0, lbrt);

            let main_func = Function::new("_start", &main_code_file, main_code);
            let init_func = Function::new("_init", "multiple files", init_code);

            let final_sym_instructions = Linker::relocate_code(main_func, init_func, functions)?;

            let mut final_instructions = Vec::with_capacity(final_sym_instructions.len());

            for instr in final_sym_instructions {
                let final_instr = syminstr_to_instr(instr, &mut argument_section)?;

                final_instructions.push(final_instr);
            }

            let addr_bytes = argument_section.get_addr_bytes();
            let mut text_section = CodeSection::new(SectionType::MAIN, addr_bytes);

            for instr in final_instructions {
                text_section.add(instr);
            }

            code_sections.push(text_section);
        } else {
            // This will build a shared library

            todo!();
        }

        debug_section = DebugSection::new(1);

        debug_section.add(DebugEntry::new(1, vec![ (0, 2) ]));

        ksm_file = KSMFile::new(argument_section, code_sections, debug_section);

        Ok(ksm_file)
    }

    /// Returns the string that is contained in the .comment section of the object file, or an empty string if there was none
    pub fn get_comment(object_file: &KOFile) -> LinkResult<String> {
        // Loop through each string table in the file
        for str_tab in object_file.get_string_tables().iter() {
            // Check if it is a .comment
            if str_tab.name() == ".comment" {
                // Get the first non-null string
                let first_string = str_tab.get(1)?;

                return Ok(first_string.to_owned());
            }
        }

        Ok(String::new())
    }

    /// Returns the source file name as a string, if none exists, an empty string is returned
    pub fn get_file_name(object_file: &KOFile) -> LinkResult<String> {
        // Loop through each symbol in the symbol table
        for sym in object_file.get_symtab().get_symbols().iter() {
            // Check if it is a FILE symbol
            if sym.get_type() == SymbolType::FILE {
                return Ok(sym.name().to_owned());
            }
        }

        Ok(String::new())
    }

    /// Turns RelInstructions using the symbol table into SymInstrs so that they don't have to rely on symbol table indexes
    pub fn rel_to_sym(
        object_file: &KOFile,
        file_name: &str,
        rel_instrs: &Vec<RelInstruction>,
    ) -> Result<Vec<SymInstr>, Box<dyn Error>> {
        let mut sym_instrs = Vec::new();

        for instr in rel_instrs {
            let opcode = instr.get_opcode();
            let mut operands = Vec::with_capacity(instr.get_operands().len());

            for op in instr.get_operands() {
                let symbol = object_file.get_symtab().get(*op as usize)?;

                match symbol.get_type() {
                    SymbolType::NOTYPE => {
                        operands.push(Operand::Value(symbol.value().clone()));
                    }
                    SymbolType::FUNC => {
                        operands.push(Operand::FuncRef(symbol.name().to_owned()));
                    }
                    t => {
                        return Err(
                            LinkError::InvalidInstrSymbolTypeError(file_name.into(), t).into()
                        )
                    }
                }
            }

            sym_instrs.push(SymInstr::new(opcode, operands));
        }

        Ok(sym_instrs)
    }

    /// Searches through the object file's symbol table for a function symbol with a section index that matches the given index
    pub fn get_func_sym(
        object_file: &KOFile,
        file_name: &str,
        section_index: i32,
    ) -> LinkResult<Symbol> {
        let sym = object_file
            .get_symtab()
            .get_symbols()
            .iter()
            .find(|s| s.get_section_index() as i32 == section_index);
        match sym {
            Some(s) => Ok(s.to_owned()),
            None => {
                Err(LinkError::FuncSymbolNotFoundError(file_name.to_owned(), section_index).into())
            }
        }
    }

    /// Performs relocation of function sections and initialization sections to form the text of the final executable
    pub fn relocate_code(
        main_code: Function,
        init_code: Function,
        functions: Vec<Function>,
    ) -> LinkResult<Vec<SymInstr>> {
        let mut output = Vec::new();
        let func_map = Linker::get_func_locations(&main_code, &init_code, &functions);

        let mut init_rel = Linker::resolve_func_refs(&init_code, &func_map)?;
        let mut main_rel = Linker::resolve_func_refs(&main_code, &func_map)?;

        output.append(&mut init_rel);
        output.append(&mut main_rel);

        for func in functions {
            output.append(&mut Linker::resolve_func_refs(&func, &func_map)?);
        }

        Ok(output)
    }

    /// This function will resolve any references to functions within another function
    /// This returns a vector of SymInstr where no operands will be function references
    pub fn resolve_func_refs(
        func: &Function,
        func_map: &HashMap<String, u32>,
    ) -> LinkResult<Vec<SymInstr>> {
        let mut new_instr = Vec::with_capacity(func.instr.len());

        for instr in &func.instr {
            let opcode = instr.opcode;

            let mut new_operands = Vec::with_capacity(instr.operands.len());

            for operand in &instr.operands {
                let new_operand = match operand {
                    Operand::Value(v) => Operand::Value(v.clone()),
                    Operand::FuncRef(f) => {
                        let func_location = func_map.get(f);

                        let func_location = match func_location {
                            Some(v) => *v,
                            None => {
                                return Err(LinkError::UndefinedReferenceToFunc(
                                    func.file.to_owned(),
                                    f.to_owned(),
                                )
                                .into());
                            }
                        };

                        let lc_string = format!("@{:0>4}", func_location);

                        Operand::Value(KOSValue::STRING(lc_string))
                    }
                };

                new_operands.push(new_operand);
            }

            new_instr.push(SymInstr::new(opcode, new_operands));
        }

        Ok(new_instr)
    }

    /// Does a run through all of the provided code, and returns a map of function names to locations in the code
    /// This assumes that the output will be an executable
    pub fn get_func_locations(
        main_code: &Function,
        init_code: &Function,
        functions: &Vec<Function>,
    ) -> HashMap<String, u32> {
        let mut map = HashMap::new();

        let mut lc = 1;

        lc += get_real_section_len(&init_code.instr);
        lc += get_real_section_len(&main_code.instr);

        for func in functions.iter() {
            let func_name = func.id.to_owned();

            map.insert(func_name, lc);

            lc += get_real_section_len(&func.instr);
        }

        map
    }
}

/// Converts a SymInstr to an Instr with the help of the argument section
fn syminstr_to_instr(syminstr: SymInstr, arg_section: &mut ArgumentSection) -> LinkResult<Instr> {
    let opcode = syminstr.opcode;
    let mut operands = Vec::with_capacity(syminstr.operands.len());

    for operand in syminstr.operands {
        let op_value = match operand {
            Operand::Value(v) => v,
            _ => unreachable!()
        };

        let op_addr = arg_section.add( op_value )?;

        operands.push(op_addr as u32);
    }

    Ok(Instr::new(opcode, operands))
}

/// Finds the number of instructions in a section, but ignores label reset instructions
fn get_real_section_len(instrs: &Vec<SymInstr>) -> u32 {
    let mut length = 0;

    for instr in instrs {
        if instr.opcode != 0xf0 {
            length += 1;
        }
    }

    length
}

/// Returns the fewest number of bytes that are required to hold the given value
#[allow(dead_code)]
fn fewest_bytes_to_hold(value: u32) -> u8 {
    if value < 255 {
        1
    } else if value < 65535 {
        2
    } else if value < 1677215 {
        3
    } else {
        4
    }
}
