//! Media identifier type for tracking media files

use uuid::Uuid;
use std::fmt;

/// Unique identifier for a media file
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MediaId(Uuid);

impl MediaId {
    /// Generate a new unique MediaId
    pub fn new() -> Self {
        MediaId(Uuid::new_v4())
    }

    /// Parse a MediaId from a string representation
    pub fn from_string(s: &str) -> Result<Self, uuid::Error> {
        let uuid = Uuid::parse_str(s)?;
        Ok(MediaId(uuid))
    }
}

impl fmt::Display for MediaId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl Default for MediaId {
    fn default() -> Self {
        Self::new()
    }
}
