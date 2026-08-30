use std::path::PathBuf;

#[derive(Debug)]
pub enum GraphiaError {
    Io { path: PathBuf, message: String },
    Parse { file: String, message: String },
    Storage { message: String },
    InvalidArgument(String),
}

impl std::fmt::Display for GraphiaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, message } => write!(f, "io error at {}: {message}", path.display()),
            Self::Parse { file, message } => write!(f, "parse error in {file}: {message}"),
            Self::Storage { message } => write!(f, "storage error: {message}"),
            Self::InvalidArgument(msg) => write!(f, "invalid argument: {msg}"),
        }
    }
}

impl std::error::Error for GraphiaError {}

pub type Result<T> = std::result::Result<T, GraphiaError>;
