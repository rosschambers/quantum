// Hand-written TypeScript mirror of the shell capture domain DTO in
// `src/domain/src/shell_capture.rs`. There is no TypeScript codegen; this
// interface tracks the Rust type field-for-field using serde's snake_case
// field names. Callers run a command with
// `client.call('shell.run_capture', { command })`, which resolves to a
// `ShellCaptureResult`.

/**
 * The captured result of running a launcher command: its command line, the
 * standard output and standard error it produced, the process exit code, and
 * whether it was terminated because it exceeded the allotted time. Mirrors the
 * Rust `ShellCaptureResult`.
 */
export interface ShellCaptureResult {
  command: string;
  stdout: string;
  stderr: string;
  exit_code: number;
  timed_out: boolean;
}
