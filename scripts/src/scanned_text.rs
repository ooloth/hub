use std::fmt;
use std::path::PathBuf;

/// Where a piece of scanned text came from, so a refusal can say where to look.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TextOrigin {
    /// The command line itself, including inline bodies, titles and heredocs.
    Command,
    /// A file the command would post from.
    File(PathBuf),
}

impl fmt::Display for TextOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Command => write!(f, "the command text"),
            Self::File(path) => write!(f, "{}", path.display()),
        }
    }
}

/// Text that a publishing call would post, gathered for scanning.
#[derive(Debug, Clone)]
pub(crate) struct ScannedText {
    pub(crate) origin: TextOrigin,
    pub(crate) text: String,
}
