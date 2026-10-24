//! What an import did, for the message the app shows afterwards.

use std::path::PathBuf;

use crate::FileProblem;
use crate::lang::Skipped;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    /// Files imported.
    pub files: usize,
    /// Types created from the sources.
    pub created: usize,
    /// Types that existed already and were brought up to date.
    pub updated: usize,
    /// Classes and packages created for library types and names the sources do not declare.
    pub placeholders: usize,
    /// Attributes turned into associations.
    pub associations: usize,
    pub skipped: Skipped,
    pub failed: Vec<FileProblem>,
    pub guessed_encodings: Vec<(PathBuf, &'static str)>,
}

impl Report {
    /// Whether anything went wrong or was left out that the user should hear about.
    pub fn has_problems(&self) -> bool {
        !self.failed.is_empty() || !self.guessed_encodings.is_empty()
    }
}
