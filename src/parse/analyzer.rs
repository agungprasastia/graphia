use crate::error::Result;
use crate::model::Language;
use crate::parser::ParsedFile;

/// Normalized language analyzer trait for code parsing and entity extraction.
pub trait LanguageAnalyzer: Send + Sync {
    /// Returns the language supported by this analyzer.
    fn language(&self) -> Language;

    /// Parse source bytes and extract symbols, imports, and calls.
    ///
    /// # Errors
    ///
    /// Returns an error if source bytes are invalid UTF-8 or malformed.
    fn analyze(&self, path: &str, source: &[u8]) -> Result<ParsedFile>;
}
