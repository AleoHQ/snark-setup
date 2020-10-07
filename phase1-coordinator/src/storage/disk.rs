use crate::{
    environment::Environment,
    storage::{InMemory, Key, Storage, Value},
    CoordinatorError,
};

// use fs2::FileExt;
use std::{
    fs::{File, OpenOptions},
    io::BufReader,
    path::Path,
    sync::{Arc, RwLock},
};

#[derive(Debug)]
pub struct Disk {
    file: File,
    in_memory: Arc<RwLock<InMemory>>,
}

impl Storage for Disk {
    /// Loads a new instance of `Disk`.
    #[inline]
    fn load(environment: &Environment) -> Result<Self, CoordinatorError> {
        // Fetch the storage base directory.
        let base_directory = environment.local_base_directory();
        // Fetch the storage file path.
        let storage_file = format!("{}/storage.json", base_directory);
        {
            // Check the base directory exists.
            if !Path::new(base_directory).exists() {
                // Create the base directory if it does not exist.
                std::fs::create_dir_all(base_directory).expect("unable to create the base directory");
            }
            // Check that the storage file exists. If not, create a new storage file.
            if !Path::new(&storage_file).exists() {
                // Create and store a new instance of `InMemory`.
                let file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .open(&storage_file)
                    .expect("unable to open the storage file");
                serde_json::to_writer_pretty(file, &InMemory::load(environment)?)?;
            }
        }

        // Open the file in read-only mode and read the JSON contents
        // of the file as an instance of `InMemory`.
        let in_memory: InMemory = serde_json::from_reader(BufReader::new(
            OpenOptions::new()
                .read(true)
                .open(&storage_file)
                .expect("unable to open the storage file"),
        ))?;

        // Create a writer.
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&storage_file)
            .expect("unable to open the storage file");

        Ok(Self {
            file,
            in_memory: Arc::new(RwLock::new(in_memory)),
        })
    }

    /// Stores an instance of `Disk`.
    /// If successful, returns `true`. Otherwise, returns `false`.
    #[inline]
    fn save(&mut self) -> bool {
        // TODO (howardwu): Fix storage key and value serialization before proceeding.
        true
    }

    /// Stores an instance of `Disk`.
    /// If successful, returns `true`. Otherwise, returns `false`.
    #[inline]
    fn save_backup(&mut self, _tag: &str) -> bool {
        // TODO (howardwu): Fix storage key and value serialization before proceeding.
        true
    }

    /// Returns the value reference for a given key from storage, if it exists.
    #[inline]
    fn get(&self, key: &Key) -> Option<Value> {
        match self.in_memory.read() {
            Ok(in_memory) => in_memory.get(key).clone(),
            _ => None,
        }
    }

    /// Returns `true` if a given key exists in storage. Otherwise, returns `false`.
    #[inline]
    fn contains_key(&self, key: &Key) -> bool {
        match self.in_memory.read() {
            Ok(in_memory) => in_memory.contains_key(key),
            _ => false,
        }
    }

    /// Inserts a new key value pair into storage,
    /// updating the current value for a given key if it exists.
    /// If successful, returns `true`. Otherwise, returns `false`.
    #[inline]
    fn insert(&mut self, key: Key, value: Value) -> bool {
        match self.in_memory.write() {
            Ok(mut in_memory) => in_memory.insert(key, value),
            _ => false,
        }
    }

    /// Removes a value from storage for a given key.
    /// If successful, returns `true`. Otherwise, returns `false`.
    #[inline]
    fn remove(&mut self, key: &Key) -> bool {
        match self.in_memory.write() {
            Ok(mut in_memory) => in_memory.remove(key),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        storage::{Disk, Storage},
        testing::prelude::*,
    };

    use std::{collections::HashMap, fs::OpenOptions, io::BufReader, path::Path};

    fn load_test() -> anyhow::Result<()> {
        let environment = &TEST_ENVIRONMENT_3;

        // Create a new instance.
        let _storage = Disk::load(&environment)?;

        // Check the base directory exists.
        let base_directory = environment.local_base_directory();
        assert!(Path::new(base_directory).exists());

        // Check the storage file path exists.
        let storage_file = format!("{}/storage.json", base_directory);
        assert!(Path::new(&storage_file).exists());

        // Open the file in read-only mode with buffer.
        let reader = BufReader::new(
            OpenOptions::new()
                .read(true)
                .open(&storage_file)
                .expect("unable to open the storage file"),
        );

        // Read the JSON contents of the file.
        let in_memory: HashMap<String, String> = serde_json::from_reader(reader)?;

        // Check that the storage key exists.
        assert!(in_memory.contains_key("storage"));

        Ok(())
    }

    #[test]
    #[serial]
    #[ignore]
    fn test_load() {
        clear_test_transcript();
        load_test().unwrap();
    }
}
