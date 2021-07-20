use std::io::{Read, Write};

use kerbalobjects::{
    kofile::{
        sections::SectionIndex,
        symbols::{KOSymbol, ReldEntry},
        Instr, KOFile,
    },
    FromBytes, KOSValue, Opcode, ToBytes,
};
use klinker::{driver::Driver, CLIConfig};

#[test]
fn link_with_locals() {
    write_main();
    write_floatlib();
    write_intlib();

    let mut buffer = Vec::with_capacity(2048);
    let mut main_file =
        std::fs::File::open("./tests/local/main.ko").expect("Error opening main.ko");

    main_file
        .read_to_end(&mut buffer)
        .expect("Error reading main.ko");

    let mut buffer_iter = buffer.iter().peekable();

    let main_ko = KOFile::from_bytes(&mut buffer_iter, false).expect("Error reading KO file");

    buffer.clear();

    let mut floatlib_file =
        std::fs::File::open("./tests/local/floatlib.ko").expect("Error opening floatlib.ko");
    floatlib_file
        .read_to_end(&mut buffer)
        .expect("Error reading floatlib.ko");

    buffer_iter = buffer.iter().peekable();

    let floatlib_ko = KOFile::from_bytes(&mut buffer_iter, false).expect("Error reading KO file");

    buffer.clear();

    let mut intlib_file =
        std::fs::File::open("./tests/local/intlib.ko").expect("Error opening intlib.ko");
    intlib_file
        .read_to_end(&mut buffer)
        .expect("Error reading intlib.ko");

    buffer_iter = buffer.iter().peekable();

    let intlib_ko = KOFile::from_bytes(&mut buffer_iter, false).expect("Error reading KO file");

    let config = CLIConfig {
        file_paths: Vec::new(),
        output_path_value: String::from("./tests/locals.ksm"),
        entry_point: String::from("_start"),
        shared: false,
        debug: true,
    };

    let mut driver = Driver::new(config.to_owned());

    driver.add_file(String::from("main.ko"), main_ko);
    driver.add_file(String::from("floatlib.ko"), floatlib_ko);
    driver.add_file(String::from("intlib.ko"), intlib_ko);

    match driver.link() {
        Ok(ksm_file) => {
            let mut file_buffer = Vec::with_capacity(2048);

            ksm_file.to_bytes(&mut file_buffer);

            let mut file =
                std::fs::File::create("./tests/locals.ksm").expect("Cannot create locals.ksm");

            file.write_all(file_buffer.as_slice())
                .expect("Cannot write locals.ksm");
        }
        Err(e) => {
            eprintln!("{}", e);
            assert!(false, "Failed to link locals");
        }
    }
}

fn write_main() {
    let mut ko = KOFile::new();

    let file_name = "./tests/local/main.ko";

    let mut data_section = ko.new_datasection(".data");
    let mut start = ko.new_funcsection("_start");
    let mut symtab = ko.new_symtab(".symtab");
    let mut symstrtab = ko.new_strtab(".symstrtab");
    let mut reld_section = ko.new_reldsection(".reld");

    let marker_value = KOSValue::ArgMarker;
    let marker_value_index = data_section.add(marker_value);

    let null_value = KOSValue::Null;
    let null_value_index = data_section.add(null_value);

    let label_1 = KOSValue::String(String::from("@0001"));
    let label_1_index = data_section.add(label_1);

    let float_num = KOSValue::Float(2.3);
    let float_num_index = data_section.add(float_num);

    let int_num = KOSValue::ScalarInt(3);
    let int_num_index = data_section.add(int_num);

    let print = KOSValue::String(String::from("print()"));
    let print_index = data_section.add(print);

    let zero = KOSValue::Int16(0);
    let zero_index = data_section.add(zero);

    let add_floats_idx = symstrtab.add("add_floats");
    let add_ints_idx = symstrtab.add("add_ints");

    let add_floats = KOSymbol::new(
        add_floats_idx,
        0,
        0,
        kerbalobjects::kofile::symbols::SymBind::Extern,
        kerbalobjects::kofile::symbols::SymType::Func,
        data_section.section_index() as u16,
    );
    let add_floats_sym = symtab.add(add_floats);

    let add_ints = KOSymbol::new(
        add_ints_idx,
        0,
        0,
        kerbalobjects::kofile::symbols::SymBind::Extern,
        kerbalobjects::kofile::symbols::SymType::Func,
        data_section.section_index() as u16,
    );
    let add_ints_sym = symtab.add(add_ints);

    // global _start
    // extern add_floats
    // extern add_ints
    //
    // .func
    // _start:
    //      push @
    //      push 2.3
    //      push 2.3
    //      call add_floats, #
    //      push @
    //      swap
    //      call #, "print()"
    //      pop
    //      push @
    //      push @
    //      push 3
    //      push 3
    //      call add_ints, #
    //      push @
    //      swap
    //      call #, "print()"
    //      pop
    //      push @
    //      push 0
    //      eop

    let reset_label = Instr::OneOp(Opcode::Lbrt, label_1_index);
    let push_marker = Instr::OneOp(Opcode::Push, marker_value_index);
    let call_floats = Instr::TwoOp(Opcode::Call, 0, null_value_index);
    let call_ints = Instr::TwoOp(Opcode::Call, 0, null_value_index);
    let push_float = Instr::OneOp(Opcode::Push, float_num_index);
    let push_int = Instr::OneOp(Opcode::Push, int_num_index);
    let call_print = Instr::TwoOp(Opcode::Call, null_value_index, print_index);
    let pop = Instr::ZeroOp(Opcode::Pop);
    let eop = Instr::ZeroOp(Opcode::Eop);
    let swap = Instr::ZeroOp(Opcode::Swap);
    let push_0 = Instr::OneOp(Opcode::Push, zero_index);

    start.add(reset_label);
    start.add(push_marker);
    start.add(push_float);
    start.add(push_float);
    let float_instr = start.add(call_floats);
    start.add(push_marker);
    start.add(swap);
    start.add(call_print);
    start.add(pop);
    start.add(push_marker);
    start.add(push_marker);
    start.add(push_int);
    start.add(push_int);
    let int_instr = start.add(call_ints);
    start.add(push_marker);
    start.add(swap);
    start.add(call_print);
    start.add(pop);
    start.add(push_marker);
    start.add(push_0);
    start.add(eop);

    let float_entry = ReldEntry::new(start.section_index(), float_instr, 0, add_floats_sym);
    let int_entry = ReldEntry::new(start.section_index(), int_instr, 0, add_ints_sym);

    reld_section.add(float_entry);
    reld_section.add(int_entry);

    let start_symbol_name_idx = symstrtab.add("_start");
    let start_symbol = KOSymbol::new(
        start_symbol_name_idx,
        0,
        start.size() as u16,
        kerbalobjects::kofile::symbols::SymBind::Global,
        kerbalobjects::kofile::symbols::SymType::Func,
        3,
    );

    let file_symbol_name_idx = symstrtab.add("main.kasm");
    let file_symbol = KOSymbol::new(
        file_symbol_name_idx,
        0,
        0,
        kerbalobjects::kofile::symbols::SymBind::Global,
        kerbalobjects::kofile::symbols::SymType::File,
        0,
    );

    symtab.add(file_symbol);
    symtab.add(start_symbol);

    ko.add_data_section(data_section);
    ko.add_func_section(start);
    ko.add_str_tab(symstrtab);
    ko.add_sym_tab(symtab);
    ko.add_reld_section(reld_section);

    let mut file_buffer = Vec::with_capacity(2048);

    ko.update_headers()
        .expect("Could not update KO headers properly");
    ko.to_bytes(&mut file_buffer);

    let mut file = std::fs::File::create(file_name)
        .expect("Output file could not be created: test-local-main.ko");

    file.write_all(file_buffer.as_slice())
        .expect("File test-global-main.ko could not be written to.");
}

fn write_floatlib() {
    let mut ko = KOFile::new();

    let file_name = "./tests/local/floatlib.ko";

    let mut data_section = ko.new_datasection(".data");
    let mut symtab = ko.new_symtab(".symtab");
    let mut symstrtab = ko.new_strtab(".symstrtab");
    let mut reld_section = ko.new_reldsection(".reld");

    let mut add_floats_func = ko.new_funcsection("add_floats");
    let mut _add_func = ko.new_funcsection("_add");

    let null_value = KOSValue::Null;
    let null_value_index = data_section.add(null_value);

    let zero = KOSValue::Int16(0);
    let zero_index = data_section.add(zero);

    let add_floats_idx = symstrtab.add("add_floats");
    let add_floats = KOSymbol::new(
        add_floats_idx,
        0,
        0,
        kerbalobjects::kofile::symbols::SymBind::Global,
        kerbalobjects::kofile::symbols::SymType::Func,
        add_floats_func.section_index() as u16,
    );
    symtab.add(add_floats);

    let _add_idx = symstrtab.add("_add");
    let _add = KOSymbol::new(
        _add_idx,
        0,
        0,
        kerbalobjects::kofile::symbols::SymBind::Local,
        kerbalobjects::kofile::symbols::SymType::Func,
        _add_func.section_index() as u16,
    );
    let _add_sym = symtab.add(_add);

    let file_symbol_name_idx = symstrtab.add("floatlib.kasm");
    let file_symbol = KOSymbol::new(
        file_symbol_name_idx,
        0,
        0,
        kerbalobjects::kofile::symbols::SymBind::Global,
        kerbalobjects::kofile::symbols::SymType::File,
        0,
    );

    // global add_floats
    //
    // .func
    // add_floats:
    //      call _add, #
    //      ret 0
    //
    // .func
    // _add:
    //      add
    //      ret 0

    let call_add = Instr::TwoOp(Opcode::Call, 0, null_value_index);
    let ret_0 = Instr::OneOp(Opcode::Ret, zero_index);
    let add = Instr::ZeroOp(Opcode::Add);

    let call_instr = add_floats_func.add(call_add);
    add_floats_func.add(ret_0);

    _add_func.add(add);
    _add_func.add(ret_0);

    let reld_entry = ReldEntry::new(add_floats_func.section_index(), call_instr, 0, _add_sym);

    reld_section.add(reld_entry);

    symtab.add(file_symbol);

    ko.add_data_section(data_section);
    ko.add_str_tab(symstrtab);
    ko.add_sym_tab(symtab);
    ko.add_reld_section(reld_section);
    ko.add_func_section(add_floats_func);
    ko.add_func_section(_add_func);

    let mut file_buffer = Vec::with_capacity(2048);

    ko.update_headers()
        .expect("Could not update KO headers properly");
    ko.to_bytes(&mut file_buffer);

    let mut file =
        std::fs::File::create(file_name).expect("Output file could not be created: funclib.ko");

    file.write_all(file_buffer.as_slice())
        .expect("funclib.ko could not be written to.");
}

fn write_intlib() {
    let mut ko = KOFile::new();

    let file_name = "./tests/local/intlib.ko";

    let mut data_section = ko.new_datasection(".data");
    let mut symtab = ko.new_symtab(".symtab");
    let mut symstrtab = ko.new_strtab(".symstrtab");
    let mut reld_section = ko.new_reldsection(".reld");

    let mut add_ints_func = ko.new_funcsection("add_ints");
    let mut _add_func = ko.new_funcsection("_add");

    let null_value = KOSValue::Null;
    let null_value_index = data_section.add(null_value);

    let zero = KOSValue::Int16(0);
    let zero_index = data_section.add(zero);

    let add_ints_idx = symstrtab.add("add_ints");
    let add_ints = KOSymbol::new(
        add_ints_idx,
        0,
        0,
        kerbalobjects::kofile::symbols::SymBind::Global,
        kerbalobjects::kofile::symbols::SymType::Func,
        add_ints_func.section_index() as u16,
    );
    symtab.add(add_ints);

    let _add_idx = symstrtab.add("_add");
    let _add = KOSymbol::new(
        _add_idx,
        0,
        0,
        kerbalobjects::kofile::symbols::SymBind::Local,
        kerbalobjects::kofile::symbols::SymType::Func,
        _add_func.section_index() as u16,
    );
    let _add_sym = symtab.add(_add);

    let file_symbol_name_idx = symstrtab.add("floatlib.ko");
    let file_symbol = KOSymbol::new(
        file_symbol_name_idx,
        0,
        0,
        kerbalobjects::kofile::symbols::SymBind::Global,
        kerbalobjects::kofile::symbols::SymType::File,
        0,
    );

    // global add_floats
    //
    // .func
    // add_floats:
    //      call _add, #
    //      ret 0
    //
    // .func
    // _add:
    //      add
    //      nop
    //      ret 0

    let call_add = Instr::TwoOp(Opcode::Call, 0, null_value_index);
    let ret_0 = Instr::OneOp(Opcode::Ret, zero_index);
    let add = Instr::ZeroOp(Opcode::Add);
    let nop = Instr::ZeroOp(Opcode::Nop);

    let call_instr = add_ints_func.add(call_add);
    add_ints_func.add(ret_0);

    _add_func.add(add);
    _add_func.add(nop);
    _add_func.add(ret_0);

    let reld_entry = ReldEntry::new(add_ints_func.section_index(), call_instr, 0, _add_sym);

    reld_section.add(reld_entry);

    symtab.add(file_symbol);

    ko.add_data_section(data_section);
    ko.add_str_tab(symstrtab);
    ko.add_sym_tab(symtab);
    ko.add_reld_section(reld_section);
    ko.add_func_section(add_ints_func);
    ko.add_func_section(_add_func);

    let mut file_buffer = Vec::with_capacity(2048);

    ko.update_headers()
        .expect("Could not update KO headers properly");
    ko.to_bytes(&mut file_buffer);

    let mut file =
        std::fs::File::create(file_name).expect("Output file could not be created: funclib.ko");

    file.write_all(file_buffer.as_slice())
        .expect("funclib.ko could not be written to.");
}