//! Repository review (qv diff mode) domain types.
use serde::{Deserialize, Serialize};

/// What to compare. `base` defaults to "HEAD"; `target: None` means the working tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DiffSpec {
    pub repository: String,
    #[serde(default = "default_base")]
    pub base: String,
    #[serde(default)]
    pub target: Option<String>,
}

fn default_base() -> String {
    "HEAD".to_string()
}

/// One side of a changed file. `content` is absent for binary or oversized files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FileSide {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    pub blob: String,
    pub mode: String,
    pub binary: bool,
    pub too_large: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ChangedFile {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Absent when the file does not exist on that side (added / deleted / untracked).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<FileSide>,
    /// Present only when the change set is stageable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<FileSide>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<FileSide>,
    pub untracked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ChangeSet {
    pub repository_root: String,
    pub base_label: String,
    pub target_label: String,
    /// True only when base is HEAD and target is the working tree (decision D2).
    pub stageable: bool,
    pub files: Vec<ChangedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "kind", content = "message", rename_all = "snake_case")]
pub enum ReviewError {
    #[error("not a git repository: {0}")]
    NotARepository(String),
    #[error("git is not available on quantumd's PATH: {0}")]
    GitUnavailable(String),
    #[error("git failed: {0}")]
    GitFailed(String),
    #[error("not stageable: {0}")]
    NotStageable(String),
    #[error("input/output error: {0}")]
    Io(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn change_set_round_trips_through_serde_json_with_snake_case_keys() {
        let change_set = ChangeSet {
            repository_root: "/home/user/project".to_string(),
            base_label: "HEAD".to_string(),
            target_label: "working tree".to_string(),
            stageable: true,
            files: vec![ChangedFile {
                path: "src/main.rs".to_string(),
                old_path: None,
                language: Some("rust".to_string()),
                base: Some(FileSide {
                    content: Some("fn main() {}".to_string()),
                    blob: "abc123".to_string(),
                    mode: "100644".to_string(),
                    binary: false,
                    too_large: false,
                }),
                index: None,
                target: Some(FileSide {
                    content: Some("fn main() { println!(); }".to_string()),
                    blob: "def456".to_string(),
                    mode: "100644".to_string(),
                    binary: false,
                    too_large: false,
                }),
                untracked: false,
            }],
        };

        let json = serde_json::to_value(&change_set).expect("serialize");
        assert_eq!(json["repository_root"], "/home/user/project");
        assert_eq!(json["base_label"], "HEAD");
        assert_eq!(json["target_label"], "working tree");
        assert_eq!(json["stageable"], true);
        assert_eq!(json["files"][0]["path"], "src/main.rs");
        assert_eq!(json["files"][0]["old_path"], serde_json::Value::Null);
        assert_eq!(json["files"][0]["language"], "rust");
        assert_eq!(json["files"][0]["base"]["blob"], "abc123");
        assert_eq!(json["files"][0]["untracked"], false);

        let recovered: ChangeSet = serde_json::from_value(json).expect("deserialize");
        assert_eq!(recovered, change_set);
    }

    #[test]
    fn file_side_with_no_content_omits_the_content_key() {
        let side = FileSide {
            content: None,
            blob: "0000000".to_string(),
            mode: "100644".to_string(),
            binary: true,
            too_large: false,
        };

        let json = serde_json::to_value(&side).expect("serialize");
        assert!(
            !json.as_object().expect("object").contains_key("content"),
            "content key must be omitted when None, got {json:?}"
        );
        assert_eq!(json["blob"], "0000000");
        assert_eq!(json["binary"], true);
    }

    #[test]
    fn diff_spec_defaults_base_to_head_and_target_to_none() {
        let json = serde_json::json!({ "repository": "/repo" });
        let spec: DiffSpec = serde_json::from_value(json).expect("deserialize");
        assert_eq!(spec.base, "HEAD");
        assert_eq!(spec.target, None);
    }

    #[test]
    fn review_error_serializes_with_tagged_kind() {
        let error = ReviewError::NotARepository("/not/a/repo".to_string());
        let json = serde_json::to_value(&error).expect("serialize");
        assert_eq!(json["kind"], "not_a_repository");
        assert_eq!(json["message"], "/not/a/repo");
    }
}
