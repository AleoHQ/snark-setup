use crate::{
    environment::Environment,
    storage::{Key, Storage, Value},
    CoordinatorError,
};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct InMemory {
    storage: HashMap<Key, Value>,
}

impl Storage for InMemory {
    /// Loads a new instance of `InMemory`.
    #[inline]
    fn load(_: &Environment) -> Result<Self, CoordinatorError> {
        Ok(Self {
            storage: HashMap::default(),
        })
    }

    /// Stores an instance of `InMemory`.
    /// If successful, returns `true`. Otherwise, returns `false`.
    #[inline]
    fn save(&mut self) -> bool {
        // As this storage is in memory, we can always return `true`.
        true
    }

    /// Stores an instance of `InMemory` for backup, in case of failure.
    /// If successful, returns `true`. Otherwise, returns `false`.
    #[inline]
    fn save_backup(&mut self, _tag: &str) -> bool {
        // As this storage is in memory, we can always return `true`.
        true
    }

    /// Returns the value reference for a given key from storage, if it exists.
    #[inline]
    fn get(&self, key: &Key) -> Option<Value> {
        match self.storage.get(key) {
            Some(key) => Some(key.clone()),
            None => None,
        }
    }

    /// Returns `true` if a given key exists in storage. Otherwise, returns `false`.
    #[inline]
    fn contains_key(&self, key: &Key) -> bool {
        self.storage.contains_key(key)
    }

    /// Inserts a new key value pair into storage,
    /// updating the current value for a given key if it exists.
    /// If successful, returns `true`. Otherwise, returns `false`.
    #[inline]
    fn insert(&mut self, key: Key, value: Value) -> bool {
        self.storage.insert(key, value);
        true
    }

    /// Removes a value from storage for a given key.
    /// If successful, returns `true`. Otherwise, returns `false`.
    #[inline]
    fn remove(&mut self, key: &Key) -> bool {
        self.storage.remove(key).is_some()
    }
}
