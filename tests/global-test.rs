use std::io::{Read, Write};
use std::path::PathBuf;

use kerbalobjects::ko::sections::DataIdx;
use kerbalobjects::ko::symbols::OperandIndex;
use kerbalobjects::ko::SectionIdx;
use kerbalobjects::{
    ko::{
        symbols::{KOSymbol, ReldEntry},
        Instr, KOFile,
    },
    BufferIterator, KOSValue, Opcode,
};
use klinker::{driver::Driver, CLIConfig};

#[test]
fn link_with_globals() {
    write_link_with_globals_main();
    write_link_with_globals_lib();

    let mut buffer = Vec::with_capacity(2048);
    let mut main_file =
        std::fs::File::open("./tests/global/main.ko").expect("Error opening main.ko");

    main_file
        .read_to_end(&mut buffer)
        .expect("Error reading main.ko");

    let mut buffer_iter = BufferIterator::new(&buffer);

    let main_ko = KOFile::parse(&mut buffer_iter).expect("Error reading KO file");

    buffer.clear();

    let mut lib_file = std::fs::File::open("./tests/global/lib.ko").expect("Error opening lib.ko");
    lib_file
        .read_to_end(&mut buffer)
        .expect("Error reading lib.ko");

    buffer_iter = BufferIterator::new(&buffer);

    let lib_ko = KOFile::parse(&mut buffer_iter).expect("Error reading KO file");

    let config = CLIConfig {
        input_paths: Vec::new(),
        output_path: PathBuf::from("./tests/global/globals.ksm"),
        entry_point: String::from("_start"),
        shared: false,
        debug: true,
    };

    let mut driver = Driver::new(config);

    driver.add_file(String::from("main.ko"), main_ko);
    driver.add_file(String::from("lib.ko"), lib_ko);

    match driver.link() {
        Ok(ksm_file) => {
            let mut file_buffer = Vec::with_capacity(2048);

            ksm_file.write(&mut file_buffer);

            let mut file =
                std::fs::File::create("./tests/globals.ksm").expect("Cannot create globals.ksm");

            file.write_all(file_buffer.as_slice())
                .expect("Cannot write globals.ksm");
        }
        Err(e) => {
            eprintln!("{}", e);
            panic!("Failed to link globals");
        }
    }
}

fn write_link_with_globals_main() {
    let mut ko = KOFile::new();

    let mut data_section = ko.new_data_section(".data");
    let mut start = ko.new_func_section("_start");
    let mut symtab = ko.new_symtab(".symtab");
    let mut symstrtab = ko.new_strtab(".symstrtab");
    let mut reld_section = ko.new_reld_section(".reld");

    let print_value = KOSValue::String(String::from("print()"));
    let print_value_index = data_section.add(print_value);

    let empty_value = KOSValue::String(String::from(""));
    let empty_value_index = data_section.add(empty_value);

    let marker_value = KOSValue::ArgMarker;
    let marker_value_index = data_section.add(marker_value);

    let number_symbol_name_idx = symstrtab.add("number");

    let number_symbol = KOSymbol::new(
        number_symbol_name_idx,
        DataIdx::PLACEHOLDER,
        0,
        kerbalobjects::ko::symbols::SymBind::Extern,
        kerbalobjects::ko::symbols::SymType::NoType,
        data_section.section_index(),
    );
    let number_symbol_index = symtab.add(number_symbol);

    let push_num_instr = Instr::OneOp(Opcode::Push, DataIdx::PLACEHOLDER);
    let add_instr = Instr::ZeroOp(Opcode::Add);
    let push_marker = Instr::OneOp(Opcode::Push, marker_value_index);
    let call_print = Instr::TwoOp(Opcode::Call, empty_value_index, print_value_index);

    start.add(push_marker);
    let first = start.add(push_num_instr);
    let second = start.add(push_num_instr);
    start.add(add_instr);
    start.add(call_print);

    let first_reld_entry = ReldEntry::new(
        start.section_index(),
        first,
        OperandIndex::One,
        number_symbol_index,
    );
    let second_reld_entry = ReldEntry::new(
        start.section_index(),
        second,
        OperandIndex::One,
        number_symbol_index,
    );

    reld_section.add(first_reld_entry);
    reld_section.add(second_reld_entry);

    let start_symbol_name_idx = symstrtab.add("_start");
    let start_symbol = KOSymbol::new(
        start_symbol_name_idx,
        DataIdx::PLACEHOLDER,
        start.size() as u16,
        kerbalobjects::ko::symbols::SymBind::Global,
        kerbalobjects::ko::symbols::SymType::Func,
        start.section_index(),
    );

    let file_symbol_name_idx = symstrtab.add("main.ko");
    let file_symbol = KOSymbol::new(
        file_symbol_name_idx,
        DataIdx::PLACEHOLDER,
        0,
        kerbalobjects::ko::symbols::SymBind::Global,
        kerbalobjects::ko::symbols::SymType::File,
        SectionIdx::NULL,
    );

    symtab.add(file_symbol);
    symtab.add(start_symbol);

    ko.add_data_section(data_section);
    ko.add_func_section(start);
    ko.add_str_tab(symstrtab);
    ko.add_sym_tab(symtab);
    ko.add_reld_section(reld_section);

    let mut file_buffer = Vec::with_capacity(2048);

    let ko = ko.validate().expect("Could not update KO headers properly");
    ko.write(&mut file_buffer);

    let mut file = std::fs::File::create("./tests/global/main.ko")
        .expect("Output file could not be created: main.ko");

    file.write_all(file_buffer.as_slice())
        .expect("File main.ko could not be written to.");
}

fn write_link_with_globals_lib() {
    let mut ko = KOFile::new();

    let mut data_section = ko.new_data_section(".data");
    let mut symtab = ko.new_symtab(".symtab");
    let mut symstrtab = ko.new_strtab(".symstrtab");

    let number_value = KOSValue::ScalarInt(32);
    let number_value_size = number_value.size_bytes();
    let number_value_idx = data_section.add(number_value);
    let number_symbol_name_idx = symstrtab.add("number");

    let number_symbol = KOSymbol::new(
        number_symbol_name_idx,
        number_value_idx,
        number_value_size as u16,
        kerbalobjects::ko::symbols::SymBind::Global,
        kerbalobjects::ko::symbols::SymType::NoType,
        data_section.section_index(),
    );
    symtab.add(number_symbol);

    let file_symbol_name_idx = symstrtab.add("lib.ko");
    let file_symbol = KOSymbol::new(
        file_symbol_name_idx,
        DataIdx::PLACEHOLDER,
        0,
        kerbalobjects::ko::symbols::SymBind::Global,
        kerbalobjects::ko::symbols::SymType::File,
        SectionIdx::NULL,
    );

    symtab.add(file_symbol);

    ko.add_data_section(data_section);
    ko.add_str_tab(symstrtab);
    ko.add_sym_tab(symtab);

    let mut file_buffer = Vec::with_capacity(2048);

    let ko = ko.validate().expect("Could not update KO headers properly");
    ko.write(&mut file_buffer);

    let mut file = std::fs::File::create("./tests/global/lib.ko")
        .expect("Output file could not be created: lib.ko");

    file.write_all(file_buffer.as_slice())
        .expect("File lib.ko could not be written to.");
}
