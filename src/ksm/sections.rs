use super::{Instr, KSMFileWriter};
use crate::KOSValue;
use std::{collections::HashMap, error::Error};

use super::KSMResult;

#[derive(PartialEq, Eq, Debug, Copy, Clone)]
pub enum SectionType {
    FUNCTION,
    INITIALIZATION,
    MAIN,
}

pub struct CodeSection {
    section_type: SectionType,
    instructions: Vec<Instr>,
    addr_bytes: u32,
    size: u32,
}

impl CodeSection {
    pub fn new(section_type: SectionType, addr_bytes: u32) -> CodeSection {
        CodeSection {
            section_type,
            instructions: Vec::new(),
            addr_bytes,
            size: 0,
        }
    }

    pub fn add(&mut self, instr: Instr) {
        let instr_size = 1 + instr.num_operands() * self.addr_bytes;
        self.size += instr_size;
        self.instructions.push(instr);
    }

    pub fn write(&self, writer: &mut KSMFileWriter) -> Result<(), Box<dyn Error>> {

        // Starting delimiters
        match self.section_type {
            SectionType::FUNCTION => {
                writer.write(b'%')?;
                writer.write(b'F')?;
            }
            SectionType::INITIALIZATION => {
                writer.write(b'%')?;
                writer.write(b'I')?;
            }
            SectionType::MAIN => {
                writer.write(b'%')?;
                writer.write(b'M')?;
            }
        }

        for instr in self.instructions.iter() {
            instr.write(self.addr_bytes, writer)?;
        }

        Ok(())
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    pub fn get_type(&self) -> SectionType {
        self.section_type
    }
}

pub struct DebugSection {
    range_size: u8,
    debug_entries: Vec<DebugEntry>,
}

impl DebugSection {
    pub fn new(range_size: u8) -> DebugSection {
        DebugSection {
            range_size,
            debug_entries: Vec::new(),
        }
    }

    pub fn add(&mut self, entry: DebugEntry) {
        self.debug_entries.push(entry);
    }

    pub fn write(&self, writer: &mut KSMFileWriter) -> Result<(), Box<dyn Error>> {
        writer.write(b'%')?;

        writer.write(b'D')?;

        writer.write(self.range_size)?;

        for entry in self.debug_entries.iter() {
            entry.write(self.range_size, writer)?;
        }

        Ok(())
    }
}

pub struct DebugEntry {
    pub line_number: u16,
    pub num_ranges: u8,
    pub ranges: Vec<(u32, u32)>,
}

impl DebugEntry {
    pub fn new(line_number: u16, ranges: Vec<(u32, u32)>) -> DebugEntry {
        DebugEntry {
            line_number,
            num_ranges: ranges.len() as u8,
            ranges,
        }
    }

    pub fn write(&self, range_size: u8, writer: &mut KSMFileWriter) -> Result<(), Box<dyn Error>> {
        writer.write_uint16(self.line_number)?;

        writer.write(self.num_ranges)?;

        for (range_start, range_end) in self.ranges.iter() {
            writer.write_variable(range_size, *range_start)?;
            writer.write_variable(range_size, *range_end)?;
        }

        Ok(())
    }
}

pub struct ArgumentSection {
    addr_bytes: u8,
    argument_list: Vec<KOSValue>,
    index_to_addr: HashMap<u32, u32>,
    size: usize,
}

pub struct KOSArgument {}

impl KOSArgument {
    pub fn write_kosval(
        value: &KOSValue,
        writer: &mut KSMFileWriter,
    ) -> Result<(), Box<dyn Error>> {
        match value {
            KOSValue::NULL => writer.write(0)?,
            KOSValue::BOOL(b) => {
                writer.write(1)?;
                writer.write_boolean(*b)?;
            }
            KOSValue::BYTE(b) => {
                writer.write(2)?;
                writer.write_byte(*b)?;
            }
            KOSValue::INT16(i) => {
                writer.write(3)?;
                writer.write_int16(*i)?;
            }
            KOSValue::INT32(i) => {
                writer.write(4)?;
                writer.write_int32(*i)?;
            }
            KOSValue::FLOAT(f) => {
                writer.write(5)?;
                writer.write_float(*f)?;
            }
            KOSValue::DOUBLE(d) => {
                writer.write(6)?;
                writer.write_double(*d)?;
            }
            KOSValue::STRING(s) => {
                writer.write(7)?;
                writer.write_kos_string(s)?;
            }
            KOSValue::ARGMARKER => writer.write(8)?,
            KOSValue::SCALARINT(i) => {
                writer.write(9)?;
                writer.write_int32(*i)?;
            }
            KOSValue::SCALARDOUBLE(d) => {
                writer.write(10)?;
                writer.write_double(*d)?;
            }
            KOSValue::BOOLEANVALUE(b) => {
                writer.write(11)?;
                writer.write_boolean(*b)?;
            }
            KOSValue::STRINGVALUE(s) => {
                writer.write(12)?;
                writer.write_kos_string(s)?;
            }
        }

        Ok(())
    }
}

impl ArgumentSection {
    pub fn new() -> ArgumentSection {
        ArgumentSection {
            addr_bytes: 4,
            argument_list: Vec::new(),
            index_to_addr: HashMap::new(),
            size: 3,
        }
    }

    /// Adds a KOSValue to the argument section, but checks if the argument already exists
    /// Returns the index into the argument section that this argument is at
    pub fn add(&mut self, value: KOSValue) -> KSMResult<usize> {
        Ok(if self.argument_list.contains(&value) {
            let mut arg_index = self.argument_list.iter();
            let index = arg_index.position(|v| *v == value).unwrap();

            *self.index_to_addr.get(&(index as u32)).unwrap() as usize
        } else {
            let addr = self.size;

            self.size += value.size() as usize;

            self.argument_list.push(value);

            self.index_to_addr
                .insert(self.argument_list.len() as u32 - 1, addr as u32);

            addr
        })
    }

    /// Unconditionally adds a KOSValue to the argument section
    /// Returns the index into the argument section that this argument is at
    pub fn add_no_check(&mut self, value: KOSValue) -> usize {
        let addr = self.size;

        self.size += value.size() as usize;

        self.argument_list.push(value);

        self.index_to_addr
            .insert(self.argument_list.len() as u32 - 1, addr as u32);

        addr
    }

    pub fn write(&mut self, writer: &mut KSMFileWriter) -> Result<(), Box<dyn Error>> {
        writer.write(b'%')?;
        writer.write(b'A')?;

        writer.write(self.get_addr_bytes() as u8)?;

        for value in self.argument_list.iter() {
            KOSArgument::write_kosval(value, writer)?;
        }

        Ok(())
    }

    pub fn get_addr(&self, index: u32) -> Result<u32, Box<dyn Error>> {
        match self.index_to_addr.get(&index) {
            Some(addr) => Ok(*addr),
            None => Err(format!("Cannot find index {} in argument section", index).into()),
        }
    }

    pub fn get_addr_bytes(&mut self) -> u32 {
        self.addr_bytes = 1;

        if self.size > 255 {
            self.addr_bytes += 1;
        }
        if self.size > 65535 {
            self.addr_bytes += 1;
        }
        if self.size > 1677215 {
            self.addr_bytes += 1;
        }

        self.addr_bytes as u32
    }
}
