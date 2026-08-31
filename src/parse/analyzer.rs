use crate::error::Result;
use crate::model::Language;
use crate::parser::ParsedFile;

pub trait LanguageAnalyzer: Send + Sync {
    fn language(&self) -> Language;
    fn analyze(&self, path: &str, source: &[u8]) -> Result<ParsedFile>;
}
