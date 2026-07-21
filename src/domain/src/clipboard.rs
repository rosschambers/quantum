//! Clipboard-history value types for the domain layer.
//!
//! No imports from other workspace crates and no input/output. A clipboard
//! entry is one of three kinds (text, image, binary), each carrying a set of
//! common fields (identity, creation time, byte size) plus kind-specific
//! payload references. Blob-backed kinds (image, binary) store only a path to
//! the on-disk blob here; the bytes live beside the JSON metadata store.

use serde::{Deserialize, Serialize};

/// Errors produced by the clipboard subsystem's persistence and value types.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ClipboardError {
    #[error("persistence error: {0}")]
    Persistence(String),
    #[error("clipboard entry not found: {0}")]
    NotFound(String),
}

/// A single clipboard-history entry. Serialized internally tagged on a `kind`
/// field in snake_case (`text` / `image` / `binary`). Every variant carries the
/// common `id`, `created_unix`, and `size_bytes` fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClipboardEntry {
    /// A plain-text clipboard entry. `preview` is a short single-line summary
    /// for display; `full` is the complete copied text.
    Text {
        id: String,
        created_unix: u64,
        size_bytes: u64,
        preview: String,
        full: String,
    },
    /// An image clipboard entry. `preview_thumb` is a `data:` URI thumbnail;
    /// `blob_path` points at the full-resolution bytes on disk.
    Image {
        id: String,
        created_unix: u64,
        size_bytes: u64,
        preview_thumb: String,
        blob_path: String,
        width: u32,
        height: u32,
    },
    /// A binary clipboard entry of an arbitrary MIME type. `blob_path` points at
    /// the copied bytes on disk.
    Binary {
        id: String,
        created_unix: u64,
        size_bytes: u64,
        mime: String,
        blob_path: String,
    },
}

impl ClipboardEntry {
    /// The entry's stable identifier, common to every variant.
    pub fn id(&self) -> &str {
        match self {
            ClipboardEntry::Text { id, .. }
            | ClipboardEntry::Image { id, .. }
            | ClipboardEntry::Binary { id, .. } => id,
        }
    }

    /// The Unix timestamp (seconds) at which the entry was captured, common to
    /// every variant.
    pub fn created_unix(&self) -> u64 {
        match self {
            ClipboardEntry::Text { created_unix, .. }
            | ClipboardEntry::Image { created_unix, .. }
            | ClipboardEntry::Binary { created_unix, .. } => *created_unix,
        }
    }

    /// The size in bytes of the entry's payload, common to every variant.
    pub fn size_bytes(&self) -> u64 {
        match self {
            ClipboardEntry::Text { size_bytes, .. }
            | ClipboardEntry::Image { size_bytes, .. }
            | ClipboardEntry::Binary { size_bytes, .. } => *size_bytes,
        }
    }

    /// The on-disk blob path for blob-backed kinds (image, binary), or `None`
    /// for text entries which store their payload inline.
    pub fn blob_path(&self) -> Option<&str> {
        match self {
            ClipboardEntry::Text { .. } => None,
            ClipboardEntry::Image { blob_path, .. } | ClipboardEntry::Binary { blob_path, .. } => {
                Some(blob_path)
            }
        }
    }
}

/// The full persisted state of the clipboard subsystem: the ordered list of
/// entries. `#[serde(default)]` lets partial JSON (including `{}`) deserialize
/// to an empty list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ClipboardData {
    pub entries: Vec<ClipboardEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_entry() -> ClipboardEntry {
        ClipboardEntry::Text {
            id: "c1".to_string(),
            created_unix: 1700,
            size_bytes: 5,
            preview: "hello".to_string(),
            full: "hello".to_string(),
        }
    }

    fn image_entry() -> ClipboardEntry {
        ClipboardEntry::Image {
            id: "c2".to_string(),
            created_unix: 1800,
            size_bytes: 4096,
            preview_thumb: "data:image/png;base64,AAAA".to_string(),
            blob_path: "/state/quantum/clipboard/c2.bin".to_string(),
            width: 64,
            height: 48,
        }
    }

    fn binary_entry() -> ClipboardEntry {
        ClipboardEntry::Binary {
            id: "c3".to_string(),
            created_unix: 1900,
            size_bytes: 1024,
            mime: "application/pdf".to_string(),
            blob_path: "/state/quantum/clipboard/c3.bin".to_string(),
        }
    }

    #[test]
    fn text_entry_round_trips() {
        let entry = text_entry();
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["kind"], "text");
        assert_eq!(json["full"], "hello");
        let back: ClipboardEntry = serde_json::from_value(json).unwrap();
        assert_eq!(back, entry);
    }

    #[test]
    fn image_entry_round_trips() {
        let entry = image_entry();
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["kind"], "image");
        assert_eq!(json["width"], 64);
        let back: ClipboardEntry = serde_json::from_value(json).unwrap();
        assert_eq!(back, entry);
    }

    #[test]
    fn binary_entry_round_trips() {
        let entry = binary_entry();
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["kind"], "binary");
        assert_eq!(json["mime"], "application/pdf");
        let back: ClipboardEntry = serde_json::from_value(json).unwrap();
        assert_eq!(back, entry);
    }

    #[test]
    fn accessors_return_common_fields() {
        assert_eq!(text_entry().id(), "c1");
        assert_eq!(text_entry().created_unix(), 1700);
        assert_eq!(text_entry().size_bytes(), 5);
        assert_eq!(text_entry().blob_path(), None);
        assert_eq!(image_entry().id(), "c2");
        assert_eq!(
            image_entry().blob_path(),
            Some("/state/quantum/clipboard/c2.bin")
        );
        assert_eq!(binary_entry().created_unix(), 1900);
        assert_eq!(
            binary_entry().blob_path(),
            Some("/state/quantum/clipboard/c3.bin")
        );
    }

    #[test]
    fn clipboard_data_default_is_empty() {
        let data = ClipboardData::default();
        assert!(data.entries.is_empty());
    }

    #[test]
    fn clipboard_data_empty_json_is_default() {
        let from_empty: ClipboardData = serde_json::from_str("{}").unwrap();
        assert_eq!(from_empty, ClipboardData::default());
    }

    #[test]
    fn clipboard_data_round_trips_with_entries() {
        let data = ClipboardData {
            entries: vec![text_entry(), image_entry(), binary_entry()],
        };
        let json = serde_json::to_string(&data).unwrap();
        let back: ClipboardData = serde_json::from_str(&json).unwrap();
        assert_eq!(back, data);
        assert_eq!(back.entries.len(), 3);
    }

    #[test]
    fn clipboard_error_variants_display() {
        assert_eq!(
            ClipboardError::Persistence("disk".to_string()).to_string(),
            "persistence error: disk"
        );
        assert_eq!(
            ClipboardError::NotFound("c9".to_string()).to_string(),
            "clipboard entry not found: c9"
        );
    }
}
