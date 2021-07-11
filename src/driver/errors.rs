use std::{
    error::Error,
    ffi::OsString,
    fmt::{Display, Formatter},
};

use kerbalobjects::errors::ReadError;

pub type LinkResult<T> = Result<T, LinkError>;

#[derive(Debug)]
pub enum LinkError {
    IOError(OsString, std::io::ErrorKind),
    FileReadError(OsString, ReadError),
    InvalidPathError(String),
    MissingSectionError(String, String),
    MissingFileSymbolNameError(String),
    FileContextError(FileErrorContext, ProcessingError),
    FuncContextError(FuncErrorContext, ProcessingError),
    MissingFileSymbolError(String),
    MissingFunctionNameError(String, String, usize),
    StringConversionError,
    InternalError(String),
    DataIndexOverflowError,
    MissingEntryPointError(String),
    MissingInitFunctionError,
    UnresolvedExternalSymbolError(String),
}

#[derive(Debug)]
pub enum ProcessingError {
    MissingNameError(String),
    InvalidDataIndexError(usize, usize),
    InvalidSymbolIndexError(usize, usize),
    MissingSymbolNameError(usize, usize),
    InvalidSymbolDataIndexError(String, usize),
    DuplicateSymbolError(String, String),
}

impl Error for LinkError {}
impl Error for ProcessingError {}

impl Display for LinkError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            LinkError::IOError(file_name, error_kind) => {
                write!(
                    f,
                    "Link error: I/O error reading {:?}, {}",
                    file_name,
                    std::io::Error::from(*error_kind)
                )
            }
            LinkError::FileReadError(file_name, e) => {
                write!(f, "Link error: Error reading {:?}, {}", file_name, e)
            }
            LinkError::InvalidPathError(path) => {
                write!(f, "Link error: I/O error, path {} invalid", path)
            }
            LinkError::StringConversionError => {
                write!(f, "Link error: File name is invalid UTF-8")
            }
            LinkError::MissingSectionError(file_name, section_name) => {
                write!(
                    f,
                    "Error linking {}.\nMissing required section {}",
                    file_name, section_name
                )
            }
            LinkError::MissingFileSymbolError(file_name) => {
                write!(f, "Error linking {}.\nMissing FILE symbol", file_name)
            }
            LinkError::MissingFileSymbolNameError(file_name) => {
                write!(f, "Error linking {}.\nMissing FILE symbol name", file_name)
            }
            LinkError::FuncContextError(ctx, e) => {
                write!(
                    f,
                    "Error linking {}, in function {}:\n{}: {}",
                    ctx.file_context.input_file_name,
                    ctx.func_name,
                    ctx.file_context.source_file_name,
                    e
                )
            }
            LinkError::FileContextError(ctx, e) => {
                write!(
                    f,
                    "Error linking {}:\n{}: {}",
                    ctx.input_file_name, ctx.source_file_name, e
                )
            }
            LinkError::MissingFunctionNameError(file_name, source_file_name, section_num) => {
                write!(
                    f,
                    "Error linking {}:\n{}: Missing function name for section {}",
                    file_name, source_file_name, section_num
                )
            }
            LinkError::InternalError(message) => {
                write!(f, "Internal error: {}", message)
            }
            LinkError::DataIndexOverflowError => {
                write!(f, "All of the instruction data takes more than 4 bytes to index. The maximum instruction operand width is 4 bytes. Try to reduce file size and try again.")
            }
            LinkError::MissingEntryPointError(entry_point) => {
                write!(
                    f,
                    "Cannot create executable, missing entry point: {}.",
                    entry_point
                )
            }
            LinkError::MissingInitFunctionError => {
                write!(f, "Cannot create shared object, missing _init function.")
            }
            LinkError::UnresolvedExternalSymbolError(name) => {
                write!(
                    f,
                    "Unresolved external symbol error. External symbol \"{}\" has no definition",
                    name
                )
            }
        }
    }
}

impl Display for ProcessingError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ProcessingError::MissingNameError(object) => {
                write!(f, "{} is missing a name entry", object)
            }
            ProcessingError::InvalidDataIndexError(instr_index, data_index) => {
                write!(
                    f,
                    "Instruction number {} has invalid data index {}",
                    instr_index, data_index
                )
            }
            ProcessingError::InvalidSymbolIndexError(instr_index, symbol_index) => {
                write!(
                    f,
                    "Instruction number {} has invalid symbol index {}",
                    instr_index, symbol_index
                )
            }
            ProcessingError::MissingSymbolNameError(symbol_index, name_index) => {
                write!(
                    f,
                    "Symbol at index {} references invalid name index {}",
                    symbol_index, name_index
                )
            }
            ProcessingError::InvalidSymbolDataIndexError(symbol_index, value_index) => {
                write!(
                    f,
                    "Symbol at index {} references invalid data index {}",
                    symbol_index, value_index
                )
            }
            ProcessingError::DuplicateSymbolError(symbol_name, original_file) => {
                write!(
                    f,
                    "Multiple definitions of '{}', first defined in {}",
                    symbol_name, original_file
                )
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileErrorContext {
    pub input_file_name: String,
    pub source_file_name: String,
}

#[derive(Debug, Clone)]
pub struct FuncErrorContext {
    pub file_context: FileErrorContext,
    pub func_name: String,
}
