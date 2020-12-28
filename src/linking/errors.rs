use kerbalobjects::SymbolType;
use std::{error::Error, fmt::Display};

pub type LinkResult<T> = Result<T, Box<dyn Error>>;

#[derive(Debug)]
pub enum LinkError {
    DuplicateSymbolError(String, String, String),
    InvalidInstrSymbolTypeError(String, SymbolType),
    FuncSymbolNotFoundError(String, i32),
    UndefinedReferenceToFunc(String, String),
}

impl Error for LinkError {}

impl Display for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let prefix = "LinkError: ";

        match self {
            LinkError::DuplicateSymbolError(this_file, sym, org_file) => {
                write!(
                    f,
                    "{}Duplicate symbol {} found in file {}. First declared in {}",
                    prefix, sym, this_file, org_file
                )
            }
            LinkError::InvalidInstrSymbolTypeError(this_file, t) => {
                write!(
                    f,
                    "{}Instruction had invalid symbol of type {:?} as operand in file {}.",
                    prefix, t, this_file
                )
            }
            LinkError::FuncSymbolNotFoundError(this_file, index) => {
                write!(
                    f,
                    "{}Expected function symbol for section {} in {}. None found.",
                    prefix, index, this_file
                )
            }
            LinkError::UndefinedReferenceToFunc(this_file, func_id) => {
                write!(
                    f,
                    "{}Undefined reference to symbol {} in {}.",
                    prefix, func_id, this_file
                )
            }
        }
    }
}
