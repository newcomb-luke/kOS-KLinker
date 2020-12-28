use std::error::Error;

pub type KSMResult<T> = Result<T, Box<dyn Error>>;
