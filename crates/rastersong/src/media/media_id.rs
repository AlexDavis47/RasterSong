//! Media identifier type for tracking media files

use uuid::Uuid;

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

    /// Get the string representation of this MediaId
    pub fn to_string(&self) -> String {
        self.0.to_string()
    }
}

impl Default for MediaId {
    fn default() -> Self {
        Self::new()
    }
}
