use std::fmt::Write as _;

use sha2::{Digest, Sha256};

#[derive(Clone, Debug)]
pub struct ClipboardEntry {
    pub mimetype: String,
    pub bytes: Vec<u8>,
    pub is_favorite: bool,
}

impl ClipboardEntry {
    pub const CANONICAL_PLAIN_TEXT_MIMETYPE: &'static str = "text/plain;charset=utf-8";

    pub fn is_text_mimetype(mimetype: &str) -> bool {
        mimetype.starts_with("text/") || mimetype == "STRING" || mimetype == "UTF8_STRING"
    }

    pub fn is_plain_text_mimetype(mimetype: &str) -> bool {
        mimetype == "text/plain"
            || mimetype.starts_with("text/plain;")
            || mimetype == "STRING"
            || mimetype == "UTF8_STRING"
            || mimetype == Self::CANONICAL_PLAIN_TEXT_MIMETYPE
    }

    pub fn canonicalize_mimetype(mimetype: &str) -> &str {
        if Self::is_plain_text_mimetype(mimetype) {
            Self::CANONICAL_PLAIN_TEXT_MIMETYPE
        } else {
            mimetype
        }
    }

    pub fn new(mimetype: impl Into<String>, bytes: Vec<u8>, is_favorite: bool) -> Self {
        let mimetype = mimetype.into();
        let mimetype = if Self::is_plain_text_mimetype(&mimetype) {
            Self::CANONICAL_PLAIN_TEXT_MIMETYPE.to_string()
        } else {
            mimetype
        };
        Self {
            mimetype,
            bytes,
            is_favorite,
        }
    }

    pub fn text(text: impl Into<String>) -> Self {
        Self::new(
            Self::CANONICAL_PLAIN_TEXT_MIMETYPE,
            text.into().into_bytes(),
            false,
        )
    }

    pub fn mimetype(&self) -> &str {
        &self.mimetype
    }

    pub fn normalized_mimetype(&self) -> &str {
        Self::canonicalize_mimetype(&self.mimetype)
    }

    pub fn is_favorite(&self) -> bool {
        self.is_favorite
    }

    pub fn is_text(&self) -> bool {
        Self::is_text_mimetype(&self.mimetype)
    }

    pub fn is_image(&self) -> bool {
        self.mimetype.starts_with("image/")
    }

    pub fn get_string_value(&self) -> String {
        if self.is_image() {
            return format!("[Image {}]", self.checksum());
        }
        String::from_utf8_lossy(&self.bytes).into_owned()
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn checksum(&self) -> String {
        let digest = Sha256::digest(&self.bytes);
        let mut checksum = String::with_capacity(digest.len() * 2);
        for byte in digest {
            let _ = write!(checksum, "{byte:02x}");
        }
        checksum
    }

    pub fn equals(&self, other: &Self) -> bool {
        self.normalized_mimetype() == other.normalized_mimetype() && self.bytes == other.bytes
    }
}

impl PartialEq for ClipboardEntry {
    fn eq(&self, other: &Self) -> bool {
        self.equals(other)
    }
}

impl Eq for ClipboardEntry {}
