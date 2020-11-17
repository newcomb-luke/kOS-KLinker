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

        let number_text_bytes = main_section.size();

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

        let final_file = KSMFile::new(argument_section, vec![ main_section ], debug_section);

        Ok(final_file)
    }

}