use std::{error::Error};
use crate::{KOFile};

use crate::ksm::{ArgumentSection, CodeSection, DebugSection, SectionType, DebugEntry, KSMFile, Instr};

pub struct Linker {
}

impl Linker {

    pub fn link(object_file: KOFile) -> Result<KSMFile, Box<dyn Error>> {

        let mut object_file = object_file;

        let mut symbols = Vec::new();
        let mut data = Vec::new();
        let mut instructions = Vec::new();

        for sym in object_file.get_symtab().get_symbols() {
            symbols.push(sym.clone());
        }

        for sym in symbols.iter() {
            sym.debug_print();
        }

        for value in object_file.get_symdata().get_values() {
            data.push(value.clone());
        }

        for value in data.iter() {
            println!("{:?}", value);
        }

        let mut argument_section = ArgumentSection::new();

        for sym in symbols.iter() {
            argument_section.add(sym.value().clone());
        }

        for instr in object_file.get_main_text().unwrap().get_instructions() {
            instr.debug_print();
            instructions.push(Instr::from_rel(instr, &argument_section)?);
        }

        let mut main_section = CodeSection::new(SectionType::MAIN, argument_section.get_addr_bytes());

        for instr in instructions.iter() {
            main_section.add(instr.clone());
        }

        let mut func_sections = Vec::new();

        for section in object_file.get_subrt_sections() {
            let mut func_section = CodeSection::new(SectionType::FUNCTION, argument_section.get_addr_bytes());
            let mut func_instructions = Vec::new();

            for instr in section.get_instructions() {
                func_instructions.push(Instr::from_rel(instr, &argument_section)?);
            }

            for instr in func_instructions {
                func_section.add(instr);
            }

            func_sections.push(func_section);
        }

        let mut number_text_bytes = main_section.size();

        for section in func_sections.iter() {
            number_text_bytes += section.size();
        }

        let mut all_code_sections = Vec::new();

        for section in func_sections {
            all_code_sections.push(section);
        }

        all_code_sections.push(main_section);

        println!("{}", number_text_bytes);

        let range_size = if number_text_bytes < 255 {
            1
        } else if number_text_bytes < 65535 {
            2
        } else if number_text_bytes < 1677215 {
            3
        } else {
            4
        };

        let mut debug_section = DebugSection::new(range_size);

        debug_section.add(DebugEntry::new(1, vec![ (6, number_text_bytes + 5) ]));

        let final_file = KSMFile::new(argument_section, all_code_sections, debug_section);

        Ok(final_file)
    }

}