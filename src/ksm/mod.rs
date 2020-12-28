use flate2::{write::GzEncoder, Compression};
use std::io::prelude::*;
use std::{error::Error, fs::File};

mod sections;
pub use sections::{ArgumentSection, CodeSection, DebugEntry, DebugSection, SectionType};

mod instruction;
pub use instruction::Instr;

mod errors;
use errors::KSMResult;

// The magic number for KSM Files is 'k' 3 X E
// In hex this is: 0x6b335845, however we need it to be little-endian so it is written as:
pub static MAGIC_NUMBER: u32 = 0x4558036b;

pub struct KSMFile {
    argument_section: ArgumentSection,
    code_sections: Vec<CodeSection>,
    debug_section: DebugSection,
}

impl KSMFile {
    pub fn new(
        argument_section: ArgumentSection,
        code_sections: Vec<CodeSection>,
        debug_section: DebugSection,
    ) -> KSMFile {
        KSMFile {
            argument_section,
            code_sections,
            debug_section,
        }
    }

    pub fn write(&mut self, writer: &mut KSMFileWriter) -> Result<(), Box<dyn Error>> {
        writer.write_uint32(MAGIC_NUMBER)?;

        self.argument_section.write(writer)?;

        for code_section in self.code_sections.iter() {
            code_section.write(writer)?;
        }

        self.debug_section.write(writer)?;

        Ok(())
    }

    pub fn get_argument_section(&self) -> &ArgumentSection {
        &self.argument_section
    }

    pub fn get_debug_section(&self) -> &DebugSection {
        &self.debug_section
    }

    pub fn get_code_sections(&self) -> &Vec<CodeSection> {
        &self.code_sections
    }
}

pub struct KSMFileWriter {
    filename: String,
    current_index: usize,
    contents: Vec<u8>,
}

impl KSMFileWriter {
    pub fn new(filename: &str) -> KSMFileWriter {
        KSMFileWriter {
            filename: filename.to_owned(),
            current_index: 0,
            contents: Vec::new(),
        }
    }

    pub fn write_to_file(&mut self) -> Result<(), Box<dyn Error>> {
        let mut file = File::create(&self.filename)?;

        let mut zipped_contents: Vec<u8> = Vec::new();

        let mut encoder = GzEncoder::new(&mut zipped_contents, Compression::best());

        encoder.write_all(&self.contents)?;

        encoder.finish()?;

        file.write_all(zipped_contents.as_slice())?;

        Ok(())
    }

    /// Returns the current index of the reader into the byte vector
    pub fn get_current_index(&self) -> usize {
        self.current_index
    }

    pub fn write(&mut self, byte: u8) -> Result<(), Box<dyn Error>> {
        self.contents.push(byte);

        Ok(())
    }

    pub fn write_multiple(&mut self, bytes: &Vec<u8>) -> Result<(), Box<dyn Error>> {
        for byte in bytes {
            self.contents.push(*byte);
        }
        Ok(())
    }

    pub fn write_boolean(&mut self, b: bool) -> Result<(), Box<dyn Error>> {
        self.contents.push(b as u8);
        Ok(())
    }

    pub fn write_byte(&mut self, b: i8) -> Result<(), Box<dyn Error>> {
        self.contents.push(b as u8);
        Ok(())
    }

    pub fn write_int16(&mut self, i: i16) -> Result<(), Box<dyn Error>> {
        for b in i16::to_le_bytes(i).iter() {
            self.contents.push(*b);
        }
        Ok(())
    }

    pub fn write_uint16(&mut self, i: u16) -> Result<(), Box<dyn Error>> {
        for b in u16::to_le_bytes(i).iter() {
            self.contents.push(*b);
        }
        Ok(())
    }

    pub fn write_int32(&mut self, i: i32) -> Result<(), Box<dyn Error>> {
        for b in i32::to_le_bytes(i).iter() {
            self.contents.push(*b);
        }
        Ok(())
    }

    pub fn write_uint32(&mut self, i: u32) -> Result<(), Box<dyn Error>> {
        for b in u32::to_le_bytes(i).iter() {
            self.contents.push(*b);
        }
        Ok(())
    }

    pub fn write_float(&mut self, f: f32) -> Result<(), Box<dyn Error>> {
        for b in f32::to_le_bytes(f).iter() {
            self.contents.push(*b);
        }
        Ok(())
    }

    pub fn write_double(&mut self, d: f64) -> Result<(), Box<dyn Error>> {
        for b in f64::to_le_bytes(d).iter() {
            self.contents.push(*b);
        }
        Ok(())
    }

    pub fn write_kos_string(&mut self, s: &str) -> Result<(), Box<dyn Error>> {
        self.contents.push(s.len() as u8);
        for b in s.bytes() {
            self.contents.push(b);
        }
        Ok(())
    }

    /// Writes a NULL-TERMINATED string to the output buffer
    pub fn write_string(&mut self, s: &str) -> Result<(), Box<dyn Error>> {
        for b in s.bytes() {
            self.contents.push(b);
        }
        self.contents.push(0);
        Ok(())
    }

    pub fn write_variable(&mut self, number_bytes: u8, value: u32) -> Result<(), Box<dyn Error>> {
        let bytes = u32::to_le_bytes(value);

        for i in 0..number_bytes as usize {
            self.contents.push(bytes[i]);
        }

        Ok(())
    }
}
