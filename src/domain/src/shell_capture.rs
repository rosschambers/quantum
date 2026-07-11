use serde::{Deserialize, Serialize};

/// The captured result of running a launcher command: its command line, the
/// standard output and standard error it produced, the process exit code, and
/// whether it was terminated because it exceeded the allotted time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShellCaptureResult {
    pub command: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub timed_out: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_capture_result_json_round_trips() {
        let result = ShellCaptureResult {
            command: "echo hello".to_string(),
            stdout: "hello\n".to_string(),
            stderr: String::new(),
            exit_code: 0,
            timed_out: false,
        };
        let json = serde_json::to_string(&result).expect("serialize");
        let parsed: ShellCaptureResult = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, result);
    }
}
