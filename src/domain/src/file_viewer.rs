/// File viewer domain types and detection functions.
///
/// Provides types for file previewing: file type classification, file info structure,
/// and pure functions for detecting file types and binary content.
use serde::{Deserialize, Serialize};

/// The type of file being viewed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewerFileType {
    /// Markdown file (.md, .mdx, .markdown)
    Markdown,
    /// JSON file (.json, .jsonc, .json5)
    Json,
    /// Source code file (.ts, .py, .rs, etc.)
    Code,
    /// Plain text file (other text that passes binary detection)
    Text,
    /// Image file (.png, .jpg, .webp, etc.)
    Image,
    /// Video file (.mp4, .webm, .mkv, etc.)
    Video,
}

/// Information about a file ready for viewing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ViewerFileInfo {
    /// File content (text types) or empty string (media types)
    pub content: String,
    /// The detected file type
    pub file_type: ViewerFileType,
    /// Language hint for syntax highlighting (for code files)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Filename with extension (e.g., "readme.md")
    pub filename: String,
    /// Directory path (e.g., "/home/user/docs/")
    pub directory: String,
    /// MIME type for media files
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// File size in bytes
    pub size: u64,
    /// file:// URI for media files
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
}

/// Detects the file type from a file extension (case-insensitive).
///
/// Returns the most specific type based on the extension, or `ViewerFileType::Text`
/// for unknown extensions.
pub fn viewer_file_type_for_extension(extension: &str) -> ViewerFileType {
    let ext_lower = extension.to_lowercase();

    match ext_lower.as_str() {
        // Markdown
        "md" | "mdx" | "markdown" => ViewerFileType::Markdown,

        // JSON
        "json" | "jsonc" | "json5" => ViewerFileType::Json,

        // Code
        "ts" | "tsx" | "js" | "jsx" | "py" | "rs" | "nix" | "toml" | "yaml" | "yml" | "sh"
        | "bash" | "zsh" | "sql" | "go" | "html" | "css" | "scss" | "dockerfile" | "lua"
        | "zig" | "c" | "cpp" | "h" | "hpp" | "java" | "kt" | "swift" | "rb" | "just" => {
            ViewerFileType::Code
        }

        // Image
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "ico" | "avif" => {
            ViewerFileType::Image
        }

        // Video
        "mp4" | "webm" | "mkv" | "mov" | "avi" | "ogg" => ViewerFileType::Video,

        // Everything else defaults to text
        _ => ViewerFileType::Text,
    }
}

/// Returns the highlight.js language name for syntax highlighting, if available.
///
/// Returns `None` for text and image/video types (which do not need syntax highlighting).
pub fn language_for_extension(extension: &str) -> Option<String> {
    let ext_lower = extension.to_lowercase();

    let lang = match ext_lower.as_str() {
        "ts" | "tsx" => "typescript",
        "js" | "jsx" => "javascript",
        "py" => "python",
        "rs" => "rust",
        "nix" => "nix",
        "json" | "jsonc" | "json5" => "json",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "sh" | "bash" | "zsh" => "bash",
        "sql" => "sql",
        "go" => "go",
        "html" => "html",
        "css" => "css",
        "scss" => "scss",
        "dockerfile" => "dockerfile",
        "lua" => "lua",
        "zig" => "zig",
        "c" | "h" => "c",
        "cpp" | "hpp" => "cpp",
        "java" => "java",
        "kt" => "kotlin",
        "swift" => "swift",
        "rb" => "ruby",
        "just" => "makefile",
        "md" | "mdx" | "markdown" => "markdown",
        _ => return None,
    };

    Some(lang.to_string())
}

/// Checks if a buffer likely contains binary data.
///
/// Returns `true` if null bytes are found in the first 8192 bytes of the buffer.
pub fn is_likely_binary(buffer: &[u8]) -> bool {
    let check_limit = buffer.len().min(8192);
    buffer[..check_limit].contains(&0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_viewer_file_type_for_markdown() {
        assert_eq!(
            viewer_file_type_for_extension("md"),
            ViewerFileType::Markdown
        );
        assert_eq!(
            viewer_file_type_for_extension("mdx"),
            ViewerFileType::Markdown
        );
        assert_eq!(
            viewer_file_type_for_extension("markdown"),
            ViewerFileType::Markdown
        );
    }

    #[test]
    fn test_viewer_file_type_for_json() {
        assert_eq!(viewer_file_type_for_extension("json"), ViewerFileType::Json);
        assert_eq!(
            viewer_file_type_for_extension("jsonc"),
            ViewerFileType::Json
        );
        assert_eq!(
            viewer_file_type_for_extension("json5"),
            ViewerFileType::Json
        );
    }

    #[test]
    fn test_viewer_file_type_for_code() {
        assert_eq!(viewer_file_type_for_extension("ts"), ViewerFileType::Code);
        assert_eq!(viewer_file_type_for_extension("tsx"), ViewerFileType::Code);
        assert_eq!(viewer_file_type_for_extension("py"), ViewerFileType::Code);
        assert_eq!(viewer_file_type_for_extension("rs"), ViewerFileType::Code);
        assert_eq!(viewer_file_type_for_extension("nix"), ViewerFileType::Code);
        assert_eq!(viewer_file_type_for_extension("go"), ViewerFileType::Code);
        assert_eq!(viewer_file_type_for_extension("java"), ViewerFileType::Code);
        assert_eq!(viewer_file_type_for_extension("rb"), ViewerFileType::Code);
        assert_eq!(viewer_file_type_for_extension("just"), ViewerFileType::Code);
    }

    #[test]
    fn test_viewer_file_type_for_image() {
        assert_eq!(viewer_file_type_for_extension("png"), ViewerFileType::Image);
        assert_eq!(viewer_file_type_for_extension("jpg"), ViewerFileType::Image);
        assert_eq!(
            viewer_file_type_for_extension("jpeg"),
            ViewerFileType::Image
        );
        assert_eq!(
            viewer_file_type_for_extension("webp"),
            ViewerFileType::Image
        );
        assert_eq!(viewer_file_type_for_extension("svg"), ViewerFileType::Image);
        assert_eq!(
            viewer_file_type_for_extension("avif"),
            ViewerFileType::Image
        );
    }

    #[test]
    fn test_viewer_file_type_for_video() {
        assert_eq!(viewer_file_type_for_extension("mp4"), ViewerFileType::Video);
        assert_eq!(
            viewer_file_type_for_extension("webm"),
            ViewerFileType::Video
        );
        assert_eq!(viewer_file_type_for_extension("mkv"), ViewerFileType::Video);
        assert_eq!(viewer_file_type_for_extension("mov"), ViewerFileType::Video);
    }

    #[test]
    fn test_viewer_file_type_for_unknown_extension() {
        assert_eq!(
            viewer_file_type_for_extension("unknown"),
            ViewerFileType::Text
        );
        assert_eq!(viewer_file_type_for_extension("xyz"), ViewerFileType::Text);
        assert_eq!(viewer_file_type_for_extension(""), ViewerFileType::Text);
    }

    #[test]
    fn test_viewer_file_type_case_insensitive() {
        assert_eq!(
            viewer_file_type_for_extension("MD"),
            ViewerFileType::Markdown
        );
        assert_eq!(
            viewer_file_type_for_extension("Md"),
            ViewerFileType::Markdown
        );
        assert_eq!(viewer_file_type_for_extension("JSON"), ViewerFileType::Json);
        assert_eq!(viewer_file_type_for_extension("PY"), ViewerFileType::Code);
        assert_eq!(viewer_file_type_for_extension("Py"), ViewerFileType::Code);
        assert_eq!(viewer_file_type_for_extension("PNG"), ViewerFileType::Image);
        assert_eq!(viewer_file_type_for_extension("MP4"), ViewerFileType::Video);
    }

    #[test]
    fn test_language_for_extension_typescript() {
        assert_eq!(language_for_extension("ts"), Some("typescript".to_string()));
        assert_eq!(
            language_for_extension("tsx"),
            Some("typescript".to_string())
        );
    }

    #[test]
    fn test_language_for_extension_javascript() {
        assert_eq!(language_for_extension("js"), Some("javascript".to_string()));
        assert_eq!(
            language_for_extension("jsx"),
            Some("javascript".to_string())
        );
    }

    #[test]
    fn test_language_for_extension_various() {
        assert_eq!(language_for_extension("py"), Some("python".to_string()));
        assert_eq!(language_for_extension("rs"), Some("rust".to_string()));
        assert_eq!(language_for_extension("nix"), Some("nix".to_string()));
        assert_eq!(language_for_extension("go"), Some("go".to_string()));
        assert_eq!(language_for_extension("java"), Some("java".to_string()));
        assert_eq!(language_for_extension("rb"), Some("ruby".to_string()));
        assert_eq!(language_for_extension("just"), Some("makefile".to_string()));
    }

    #[test]
    fn test_language_for_extension_json() {
        assert_eq!(language_for_extension("json"), Some("json".to_string()));
        assert_eq!(language_for_extension("jsonc"), Some("json".to_string()));
        assert_eq!(language_for_extension("json5"), Some("json".to_string()));
    }

    #[test]
    fn test_language_for_extension_markup_and_styles() {
        assert_eq!(language_for_extension("html"), Some("html".to_string()));
        assert_eq!(language_for_extension("css"), Some("css".to_string()));
        assert_eq!(language_for_extension("scss"), Some("scss".to_string()));
        assert_eq!(language_for_extension("yaml"), Some("yaml".to_string()));
        assert_eq!(language_for_extension("yml"), Some("yaml".to_string()));
    }

    #[test]
    fn test_language_for_extension_markdown() {
        assert_eq!(language_for_extension("md"), Some("markdown".to_string()));
        assert_eq!(language_for_extension("mdx"), Some("markdown".to_string()));
    }

    #[test]
    fn test_language_for_extension_none() {
        assert_eq!(language_for_extension("png"), None);
        assert_eq!(language_for_extension("mp4"), None);
        assert_eq!(language_for_extension("txt"), None);
        assert_eq!(language_for_extension("unknown"), None);
    }

    #[test]
    fn test_is_likely_binary_with_null_bytes() {
        let binary_data = b"Some data\x00with null byte";
        assert!(is_likely_binary(binary_data));
    }

    #[test]
    fn test_is_likely_binary_without_null_bytes() {
        let text_data = b"This is just plain text with no null bytes";
        assert!(!is_likely_binary(text_data));
    }

    #[test]
    fn test_is_likely_binary_null_byte_at_start() {
        let binary_data = b"\x00starts with null";
        assert!(is_likely_binary(binary_data));
    }

    #[test]
    fn test_is_likely_binary_null_byte_at_limit() {
        let mut data = vec![b'a'; 8192];
        data[8191] = 0;
        assert!(is_likely_binary(&data));
    }

    #[test]
    fn test_is_likely_binary_null_byte_beyond_limit() {
        let mut data = vec![b'a'; 9000];
        data[8192] = 0; // Beyond the 8192 limit, should not be detected
        assert!(!is_likely_binary(&data));
    }

    #[test]
    fn test_is_likely_binary_empty_buffer() {
        let data: &[u8] = b"";
        assert!(!is_likely_binary(data));
    }

    #[test]
    fn test_viewer_file_info_serde_roundtrip() {
        let info = ViewerFileInfo {
            content: "# Hello\n\nWorld".to_string(),
            file_type: ViewerFileType::Markdown,
            language: None,
            filename: "test.md".to_string(),
            directory: "/home/user/".to_string(),
            mime_type: None,
            size: 256,
            uri: None,
        };

        let json = serde_json::to_string(&info).expect("serialize");
        let recovered: ViewerFileInfo = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(info.content, recovered.content);
        assert_eq!(info.file_type, recovered.file_type);
        assert_eq!(info.language, recovered.language);
        assert_eq!(info.filename, recovered.filename);
        assert_eq!(info.directory, recovered.directory);
        assert_eq!(info.mime_type, recovered.mime_type);
        assert_eq!(info.size, recovered.size);
        assert_eq!(info.uri, recovered.uri);
    }

    #[test]
    fn test_viewer_file_info_serde_roundtrip_with_all_fields() {
        let info = ViewerFileInfo {
            content: "print('hello')".to_string(),
            file_type: ViewerFileType::Code,
            language: Some("python".to_string()),
            filename: "script.py".to_string(),
            directory: "/tmp/".to_string(),
            mime_type: Some("text/plain".to_string()),
            size: 512,
            uri: Some("file:///tmp/script.py".to_string()),
        };

        let json = serde_json::to_string(&info).expect("serialize");
        let recovered: ViewerFileInfo = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(info.content, recovered.content);
        assert_eq!(info.file_type, recovered.file_type);
        assert_eq!(info.language, recovered.language);
        assert_eq!(info.filename, recovered.filename);
        assert_eq!(info.directory, recovered.directory);
        assert_eq!(info.mime_type, recovered.mime_type);
        assert_eq!(info.size, recovered.size);
        assert_eq!(info.uri, recovered.uri);
    }
}
