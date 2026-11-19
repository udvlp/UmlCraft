//! Astah Professional project files (`.asta`): import into a model, and reading and writing
//! their object graphs.
//!
//! An `.asta` file is a ZIP archive with one entry, `EntityStore`: a Java serialization stream
//! of Astah's model objects, preceded by a short Astah-specific header.

mod diff;
pub mod jser;
mod map;

pub use map::Imported;

use std::fs::File;
use std::io::{self, Read, Write};
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
    #[error("the Astah project is too large")]
    TooLarge,
    #[error("not an Astah project: the file is damaged")]
    Damaged,
}

/// Reads an `.asta` file into a new model.
pub fn import(path: &Path) -> Result<Imported, AstaError> {
    let graph = parse_entity_store(&read_entity_store(path)?)?;
    // The mapper follows the object graph recursively too.
    deep(|| map::import(&graph).ok_or(AstaError::NoModel))
}

/// Astah writes `0x00EE` and a store version of 1 after the standard stream header.
const STORE_MAGIC: [u8; 4] = [0x00, 0xEE, 0x00, 0x01];

/// The largest entity store read, decompressed. Astah's own sample projects need a few MB;
/// the limit only keeps a damaged or hostile size field from exhausting memory. The graph read
/// from it has a memory budget of its own (`jser::parse`).
const MAX_ENTITY_STORE: u64 = 256 << 20;

/// The raw `EntityStore` entry of an `.asta` file.
pub fn read_entity_store(path: &Path) -> Result<Vec<u8>, AstaError> {
    let mut archive = zip::ZipArchive::new(File::open(path)?)?;
    let entry = archive.by_name("EntityStore")?;
    // The declared size is only a hint: trust it neither for the allocation nor as a bound.
    let mut bytes = Vec::with_capacity(entry.size().min(64 << 20) as usize);
    entry.take(MAX_ENTITY_STORE + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_ENTITY_STORE {
        return Err(AstaError::TooLarge);
    }
    Ok(bytes)
}

/// Parses an entity store into a generic object graph.
pub fn parse_entity_store(bytes: &[u8]) -> Result<jser::Graph, AstaError> {
    if bytes.get(4..8) != Some(&STORE_MAGIC[..]) {
        return Err(AstaError::BadHeader);
    }
    deep(|| match jser::parse(bytes, STORE_MAGIC.len()) {
        Err(jser::StreamError::TooLarge) => Err(AstaError::TooLarge),
        result => Ok(result?),
    })
}

/// Writes an object graph as an entity store. Tooling, not part of the import: the
/// `asta-rewrite` example and `tests/rewrite.rs` check with it that reading loses nothing, and
/// tests can build `.asta` files from a [`jser::Graph`] with it.
pub fn write_entity_store(graph: &jser::Graph) -> Result<Vec<u8>, AstaError> {
    deep(|| Ok(jser::write(graph, &STORE_MAGIC)?))
}

/// Writes an `.asta` file with the given entity store. Tooling, as [`write_entity_store`].
pub fn write_asta(path: &Path, entity_store: &[u8]) -> Result<(), AstaError> {
    let mut archive = zip::ZipWriter::new(File::create(path)?);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    archive.start_file("EntityStore", options)?;
    archive.write_all(entity_store)?;
    archive.finish()?;
    Ok(())
}

/// The differences between two object graphs, one line each; see [`diff::diff`].
pub fn diff_graphs(a: &jser::Graph, b: &jser::Graph) -> Result<Vec<String>, AstaError> {
    deep(|| Ok(diff::diff(a, b)))
}

/// Astah's object graph is deeply nested, so reading and writing it runs on a thread with a
/// large stack. A panic on that thread means the file broke an assumption of the reader, so it
/// is reported as a damaged file rather than taking the application down.
fn deep<T: Send>(f: impl FnOnce() -> Result<T, AstaError> + Send) -> Result<T, AstaError> {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(512 << 20)
            .spawn_scoped(scope, f)
            .map_err(AstaError::Io)?
            .join()
            .unwrap_or(Err(AstaError::Damaged))
    })
}
