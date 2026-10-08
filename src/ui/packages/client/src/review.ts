// Repository review (qv diff mode) IPC types. Hand-written mirrors of the Rust
// DTOs in `src/domain/src/review.rs` (there is no codegen): field names are the
// snake_case serde names exactly as they cross IPC. Optional fields are OMITTED
// when absent (serde `skip_serializing_if`), never sent as null, so check for
// key presence rather than comparing against null.

/** What to compare. `base` defaults to "HEAD" server-side; no `target` means the working tree. */
export interface DiffSpec {
  repository: string;
  base?: string;
  target?: string | null;
}

/** One side of a changed file. `content` is absent for binary, oversized, or submodule sides. */
export interface FileSide {
  content?: string;
  blob: string;
  mode: string;
  binary: boolean;
  too_large: boolean;
}

/**
 * A changed file. A missing side means the file does not exist there (added,
 * deleted, untracked). `index` is present whenever the file is tracked and the
 * change set is stageable — compare blob ids (never content) to tell staged
 * from unstaged: base.blob !== index.blob means staged changes exist;
 * index.blob !== target.blob means unstaged changes exist.
 */
export interface ChangedFile {
  path: string;
  old_path?: string;
  language?: string;
  base?: FileSide;
  index?: FileSide;
  target?: FileSide;
  untracked: boolean;
}

/** The result of `file-viewer.changes`. Stageable only for base HEAD against the working tree. */
export interface ChangeSet {
  repository_root: string;
  base_label: string;
  target_label: string;
  stageable: boolean;
  files: ChangedFile[];
}

/** The `kind` of a repository review error (JSON-RPC codes -32020 through -32024). */
export type ReviewErrorKind =
  | 'not_a_repository'
  | 'git_unavailable'
  | 'git_failed'
  | 'not_stageable'
  | 'io';

/** Params for `file-viewer.stage`. `blob: null` stages a deletion. `mode` is 100644, 100755, or 120000. */
export interface StageParams {
  repository_root: string;
  path: string;
  blob: string | null;
  mode: string;
}

/** Params for `file-viewer.unstage`. */
export interface UnstageParams {
  repository_root: string;
  path: string;
}

/** Params for `file-viewer.fingerprint`. */
export interface FingerprintParams {
  repository_root: string;
}

/** Result of `file-viewer.fingerprint`. */
export interface FingerprintResult {
  fingerprint: string;
}
