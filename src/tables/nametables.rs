use std::slice::{Iter, IterMut};
use std::{collections::hash_map::DefaultHasher, hash::Hasher, num::NonZeroUsize};

#[derive(Debug, Clone)]
pub struct NameTableEntry<T> {
    name: String,
    value: T,
}

#[derive(Debug)]
pub struct NameTable<T> {
    hashes: Vec<u64>,
    entries: Vec<NameTableEntry<T>>,
    size: usize,
}

impl<T> NameTableEntry<T> {
    pub fn from(name: String, value: T) -> Self {
        NameTableEntry { name, value }
    }

    pub fn name(&self) -> &String {
        &self.name
    }

    pub fn value(&self) -> &T {
        &self.value
    }

    pub fn set_value(&mut self, new: T) {
        self.value = new
    }
}

impl<T> From<NameTableEntry<T>> for String {
    fn from(entry: NameTableEntry<T>) -> Self {
        entry.name
    }
}

impl<T> NameTable<T> {
    pub fn new() -> Self {
        NameTable {
            hashes: Vec::new(),
            entries: Vec::new(),
            size: 0,
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        NameTable {
            hashes: Vec::with_capacity(capacity),
            entries: Vec::with_capacity(capacity),
            size: 0,
        }
    }

    pub fn replace_at(&mut self, index: NonZeroUsize, new_value: T) -> Result<(), ()> {
        let entry = self.entries.get_mut(index.get() - 1).ok_or(())?;

        entry.set_value(new_value);

        Ok(())
    }

    pub fn replace_by_hash(&mut self, hash: u64, new_value: T) -> Result<(), ()> {
        let entry = self.get_mut_by_hash(hash).ok_or(())?;

        entry.set_value(new_value);

        Ok(())
    }

    pub fn raw_insert(&mut self, hash: u64, entry: NameTableEntry<T>) -> NonZeroUsize {
        match self.position_by_hash(hash) {
            Some(pos) => pos,
            None => {
                self.hashes.push(hash);
                self.entries.push(entry);
                self.size += 1;

                // SAFETY: This is safe because the "real" index is always equal to the size plus 1
                unsafe { NonZeroUsize::new_unchecked(self.size) }
            }
        }
    }

    pub fn insert(&mut self, entry: NameTableEntry<T>) -> NonZeroUsize {
        match self.position(&entry.name) {
            Some(pos) => pos,
            None => {
                let mut hasher = DefaultHasher::new();
                hasher.write(entry.name.as_bytes());

                let hash = hasher.finish();

                self.hashes.push(hash);
                self.entries.push(entry);
                self.size += 1;

                // SAFETY: This is safe because the "real" index is always equal to the size plus 1
                unsafe { NonZeroUsize::new_unchecked(self.size) }
            }
        }
    }

    pub fn get_hash_at(&self, index: NonZeroUsize) -> Option<&u64> {
        self.hashes.get(index.get() - 1)
    }

    pub fn get_at(&self, index: NonZeroUsize) -> Option<&NameTableEntry<T>> {
        self.entries.get(index.get() - 1)
    }

    pub fn get_at_mut(&mut self, index: NonZeroUsize) -> Option<&mut NameTableEntry<T>> {
        self.entries.get_mut(index.get() - 1)
    }

    pub fn get(&self, name: &str) -> Option<&NameTableEntry<T>> {
        let index = self.position(name)?;
        self.get_at(index)
    }

    pub fn get_by_hash(&self, hash: u64) -> Option<&NameTableEntry<T>> {
        let index = self.position_by_hash(hash)?;
        self.get_at(index)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut NameTableEntry<T>> {
        let index = self.position(name)?;
        self.get_at_mut(index)
    }

    pub fn get_mut_by_hash(&mut self, hash: u64) -> Option<&mut NameTableEntry<T>> {
        let index = self.position_by_hash(hash)?;
        self.get_at_mut(index)
    }

    pub fn position(&self, name: &str) -> Option<NonZeroUsize> {
        let mut hasher = DefaultHasher::new();
        hasher.write(name.as_bytes());
        let hash = hasher.finish();

        self.position_by_hash(hash)
    }

    pub fn position_by_hash(&self, hash: u64) -> Option<NonZeroUsize> {
        // SAFETY: This is safe because the "real" index always has the value of 1 added to it
        unsafe {
            self.hashes
                .iter()
                .position(|item| *item == hash)
                .map(|index| NonZeroUsize::new_unchecked(index + 1))
        }
    }

    pub fn contains(&self, name: &str) -> bool {
        self.position(name).is_some()
    }

    pub fn contains_hash(&self, hash: u64) -> bool {
        self.position_by_hash(hash).is_some()
    }

    pub fn entries(&self) -> Iter<NameTableEntry<T>> {
        self.entries.iter()
    }

    pub fn entries_mut(&mut self) -> IterMut<NameTableEntry<T>> {
        self.entries.iter_mut()
    }

    pub fn drain(&mut self) -> Vec<NameTableEntry<T>> {
        self.entries.drain(..).collect()
    }
}
