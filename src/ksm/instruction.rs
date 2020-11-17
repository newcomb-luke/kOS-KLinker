use std::{error::Error};

use super::{KSMFileWriter, ArgumentSection};

use crate::{RelInstruction};

/// A struct representing an instruction in the context of a KSM file.
/// The operands to this instruction should be u32's that represent an index into an ArgumentSection
#[derive(Debug, Clone)]
pub struct Instr {
    opcode: u8,
    operands: Vec<u32>
}

impl Instr {
    pub fn new(opcode: u8, operands: Vec<u32>) -> Instr {
        Instr { opcode, operands }
    }

    pub fn write(&self, addr_bytes: u32, writer: &mut KSMFileWriter) -> Result<(), Box<dyn Error>> {

        writer.write(self.opcode)?;

        for op in self.operands.iter() {
            writer.write_variable(addr_bytes as u8, *op)?;
        }

        Ok(())
    }

    pub fn num_operands(&self) -> u32 {
        self.operands.len() as u32
    }

    pub fn from_rel(rel_instr: &RelInstruction, argument_section: &ArgumentSection) -> Result<Instr, Box<dyn Error>> {

        let mut operands = Vec::new();

        for op in rel_instr.get_operands().iter() {
            operands.push( argument_section.get_addr(*op)? );
        }

        Ok(Instr::new( rel_instr.get_opcode(), operands))
    }
}