//! Read-only import of Astah Professional project files (`.asta`).
//!
//! An `.asta` file is a ZIP archive with one entry, `EntityStore`: a Java serialization stream
//! of Astah's model objects, preceded by a short Astah-specific header.

pub mod jser;
mod map;

pub use map::Imported;

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AstaError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("not an Astah project: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("not an Astah project: {0}")]
    Stream(#[from] jser::StreamError),
    #[error("not an Astah project: the entity store has a wrong header")]
    BadHeader,
    #[error("the Astah project contains no model")]
    NoModel,
}

/// Reads an `.asta` file into a new model.
pub fn import(path: &Path) -> Result<Imported, AstaError> {
    let graph = parse_entity_store(&read_entity_store(path)?)?;
    map::import(&graph).ok_or(AstaError::NoModel)
}

/// Astah writes `0x00EE` and a store version of 1 after the standard stream header.
const STORE_MAGIC: [u8; 4] = [0x00, 0xEE, 0x00, 0x01];

/// The raw `EntityStore` entry of an `.asta` file.
pub fn read_entity_store(path: &Path) -> Result<Vec<u8>, AstaError> {
    let mut archive = zip::ZipArchive::new(File::open(path)?)?;
    let mut entry = archive.by_name("EntityStore")?;
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Parses an entity store into a generic object graph.
///
/// Astah's object graph is deeply nested, so parsing runs on a thread with a large stack.
pub fn parse_entity_store(bytes: &[u8]) -> Result<jser::Graph, AstaError> {
    if bytes.get(4..8) != Some(&STORE_MAGIC[..]) {
        return Err(AstaError::BadHeader);
    }
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(512 << 20)
            .spawn_scoped(scope, || jser::parse(bytes, STORE_MAGIC.len()))
            .map_err(AstaError::Io)?
            .join()
            .expect("parser thread panicked")
            .map_err(AstaError::from)
    })
}
