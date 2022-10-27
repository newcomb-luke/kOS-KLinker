use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;
use std::slice::{Iter, IterMut};
use std::vec::Drain;

use kerbalobjects::{ko::symbols::KOSymbol, KOSValue, Opcode};

mod nametables;
pub use nametables::*;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Ord, PartialOrd)]
pub enum ContextHash {
    FuncNameHash(u64),
    FileNameHash(u64),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TempOperand {
    DataHash(u64),
    SymNameHash(u64),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TempInstr {
    ZeroOp(Opcode),
    OneOp(Opcode, TempOperand),
    TwoOp(Opcode, TempOperand, TempOperand),
}

#[derive(Debug)]
pub struct ObjectData {
    pub input_file_name: String,
    pub source_file_name: String,
    pub comment: Option<String>,
    pub symbol_name_table: NameTable<NonZeroUsize>,
    pub function_name_table: NameTable<NonZeroUsize>,
    pub function_table: FunctionTable,
    pub symbol_table: SymbolTable,
    pub data_table: DataTable,
    pub local_function_table: FunctionTable,
    pub local_symbol_table: SymbolTable,
    pub local_function_hash_map: HashMap<u64, usize>,
    pub local_function_name_table: NameTable<NonZeroUsize>,
    pub local_function_ref_vec: Vec<u64>,
}

#[derive(Debug, Clone)]
pub struct Function {
    object_data_index: usize,
    name_hash: u64,
    is_global: bool,
    instructions: Vec<TempInstr>,
}

#[derive(Debug)]
pub struct FunctionTable {
    entries: Vec<Function>,
}

#[derive(Debug)]
pub struct SymbolEntry {
    name_hash: u64,
    symbol: KOSymbol,
    ctx: ContextHash,
}

#[derive(Debug)]
pub struct MasterSymbolEntry {
    symbol: KOSymbol,
    ctx: ContextHash,
}

#[derive(Debug)]
pub struct SymbolTable {
    entries: Vec<SymbolEntry>,
}

#[derive(Debug)]
pub struct DataTable {
    hashes: Vec<u64>,
    data: Vec<KOSValue>,
}

impl Function {
    pub fn new(name_hash: u64, is_global: bool) -> Self {
        Function {
            object_data_index: 0,
            name_hash,
            is_global,
            instructions: Vec::new(),
        }
    }

    pub fn with_capacity(name_hash: u64, is_global: bool, capacity: usize) -> Self {
        Function {
            object_data_index: 0,
            name_hash,
            is_global,
            instructions: Vec::with_capacity(capacity),
        }
    }

    pub fn name_hash(&self) -> u64 {
        self.name_hash
    }

    pub fn is_global(&self) -> bool {
        self.is_global
    }

    pub fn add(&mut self, instr: TempInstr) {
        self.instructions.push(instr);
    }

    pub fn instructions(&self) -> Iter<TempInstr> {
        self.instructions.iter()
    }

    pub fn drain(&mut self) -> Vec<TempInstr> {
        self.instructions.drain(..).collect()
    }

    pub fn instruction_count(&self) -> usize {
        self.instructions.len()
    }

    pub fn set_object_data_index(&mut self, index: usize) {
        self.object_data_index = index;
    }

    pub fn object_data_index(&self) -> usize {
        self.object_data_index
    }
}

impl FunctionTable {
    pub fn new() -> Self {
        FunctionTable {
            entries: Vec::new(),
        }
    }

    pub fn add(&mut self, func: Function) {
        self.entries.push(func);
    }

    pub fn functions(&self) -> Iter<Function> {
        self.entries.iter()
    }

    pub fn functions_mut(&mut self) -> IterMut<Function> {
        self.entries.iter_mut()
    }

    pub fn drain(&mut self) -> Vec<Function> {
        self.entries.drain(..).collect()
    }

    pub fn get_by_hash(&self, hash: u64) -> Option<&Function> {
        self.entries.iter().find(|func| func.name_hash == hash)
    }
}

impl SymbolEntry {
    pub fn new(name_hash: u64, symbol: KOSymbol, ctx: ContextHash) -> Self {
        SymbolEntry {
            name_hash,
            symbol,
            ctx,
        }
    }

    pub fn name_hash(&self) -> u64 {
        self.name_hash
    }

    pub fn internal(&self) -> &KOSymbol {
        &self.symbol
    }

    pub fn internal_mut(&mut self) -> &mut KOSymbol {
        &mut self.symbol
    }

    pub fn context(&self) -> ContextHash {
        self.ctx
    }

    pub fn set_context(&mut self, new: ContextHash) {
        self.ctx = new;
    }
}

impl From<SymbolEntry> for KOSymbol {
    fn from(entry: SymbolEntry) -> Self {
        entry.symbol
    }
}

impl MasterSymbolEntry {
    pub fn new(symbol: KOSymbol, ctx: ContextHash) -> Self {
        MasterSymbolEntry { symbol, ctx }
    }

    pub fn internal(&self) -> &KOSymbol {
        &self.symbol
    }

    pub fn internal_mut(&mut self) -> &mut KOSymbol {
        &mut self.symbol
    }

    pub fn context(&self) -> ContextHash {
        self.ctx
    }
}

impl From<SymbolEntry> for MasterSymbolEntry {
    fn from(entry: SymbolEntry) -> Self {
        MasterSymbolEntry {
            symbol: entry.symbol,
            ctx: entry.ctx,
        }
    }
}

impl SymbolTable {
    pub fn new() -> Self {
        SymbolTable {
            entries: Vec::new(),
        }
    }

    pub fn add(&mut self, entry: SymbolEntry) -> NonZeroUsize {
        self.entries.push(entry);

        // SAFETY: This is safe because it is after we just added an item, it will always be >= 1
        unsafe { NonZeroUsize::new_unchecked(self.entries.len()) }
    }

    pub fn symbols(&self) -> Iter<SymbolEntry> {
        self.entries.iter()
    }

    pub fn drain(&mut self) -> Drain<SymbolEntry> {
        self.entries.drain(..)
    }

    pub fn get_by_hash(&self, hash: u64) -> Option<&SymbolEntry> {
        self.entries.iter().find(|sym| sym.name_hash == hash)
    }
}
impl DataTable {
    pub fn new() -> Self {
        DataTable {
            hashes: Vec::new(),
            data: Vec::new(),
        }
    }

    pub fn add(&mut self, value: KOSValue) -> (u64, NonZeroUsize) {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        let hash = hasher.finish();

        (
            hash,
            match self.hashes.iter().position(|item| *item == hash) {
                // SAFETY: This is safe because we add 1 to it unconditionally
                Some(pos) => unsafe { NonZeroUsize::new_unchecked(pos + 1) },
                None => {
                    self.hashes.push(hash);
                    self.data.push(value);
                    // SAFETY: This is safe because it is after we just added an item, it will always be >= 1
                    unsafe { NonZeroUsize::new_unchecked(self.hashes.len()) }
                }
            },
        )
    }

    pub fn get_at(&self, index: NonZeroUsize) -> Option<&KOSValue> {
        self.data.get(index.get() - 1)
    }

    pub fn get_by_hash(&self, hash: u64) -> Option<&KOSValue> {
        match self.hashes.iter().position(|item| item == &hash) {
            Some(pos) => self.data.get(pos),
            None => None,
        }
    }

    pub fn hash_at(&self, index: NonZeroUsize) -> Option<&u64> {
        self.hashes.get(index.get() - 1)
    }

    pub fn entries(&self) -> Iter<KOSValue> {
        self.data.iter()
    }

    pub fn hashes(&self) -> Iter<u64> {
        self.hashes.iter()
    }

    pub fn size_bytes(&self) -> usize {
        let mut size = 0;

        for value in self.data.iter() {
            size += value.size_bytes();
        }

        size
    }
}
