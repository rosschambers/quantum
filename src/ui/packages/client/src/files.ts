// Hand-written TypeScript DTOs mirroring the Rust file-explorer types from
// `quantum-domain` / `quantum-application` (serde output). There is no codegen;
// keep these shapes and their snake_case field names in exact lockstep with the
// Rust structs and enums. This file is DTO-only, matching `timer.ts` and
// `notifications.ts` — no client wrapper lives here. The file-explorer view's
// `ipc.ts` (Task 17) is the place that calls the daemon.
//
// The daemon exposes these file-explorer methods (documented here, not wrapped):
//   files.list          list entries for a directory
//   files.places        pins and mounted drives
//   files.pin           pin a directory to the sidebar
//   files.unpin         unpin a directory from the sidebar
//   files.operation     perform a FileOperation
//   files.open          open a path with its default application
//   files.open_with     open a path with a chosen application
//   files.applications  applications that can open a path
//   files.open_terminal open a terminal in a directory
//   files.preview       a preview payload for a path
//   files.search        recursively search a directory
//   files.watch         start watching a directory for changes
//   files.unwatch       stop watching a directory
//   files.sizes         start computing recursive directory sizes
//   files.cancel_sizes  stop computing recursive directory sizes
// Live changes are published on the `files.event` channel as a `FilesEvent`.

/** The kind of a file-system entry. Mirrors the Rust `FileEntryKind` enum. */
export type FileEntryKind = 'directory' | 'file' | 'symlink';

/** Broad permission category for an entry. Mirrors the Rust `PermissionClass` enum. */
export type PermissionClass = 'executable' | 'read_only' | 'root_owned' | 'normal';

/** Content classification derived from an entry. Mirrors the Rust `ContentKind` enum. */
export type ContentKind = 'image' | 'document' | 'code' | 'archive' | 'music' | 'other';

/** A single file-system entry as listed by `files.list`. */
export interface FileEntry {
  name: string;
  path: string;
  kind: FileEntryKind;
  size: number;
  recursive_size: number | null;
  modified_epoch_seconds: number;
  owner: string;
  permissions: string;
  permission_class: PermissionClass;
  symlink_target: string | null;
  content_kind: ContentKind;
}

/** A mounted drive. Mirrors the Rust `DriveInfo` struct. */
export interface DriveInfo {
  label: string;
  mount_point: string;
  total_bytes: number;
  free_bytes: number;
}

/** A pinned location in the places sidebar. Mirrors the Rust `Pin` struct. */
export interface Pin {
  label: string;
  path: string;
}

/** An application that can open a file. Mirrors the Rust `ApplicationInfo` struct. */
export interface ApplicationInfo {
  id: string;
  name: string;
}

/**
 * A file-system operation requested via `files.operate`. Discriminated union on
 * `kind` (snake_case), mirroring the Rust `FileOperation` enum.
 */
export type FileOperation =
  | { kind: 'copy'; sources: string[]; destination: string }
  | { kind: 'move'; sources: string[]; destination: string }
  | { kind: 'rename'; path: string; new_name: string }
  | { kind: 'duplicate'; path: string }
  | { kind: 'new_folder'; parent: string; name: string }
  | { kind: 'new_file'; parent: string; name: string }
  | { kind: 'trash'; paths: string[] }
  | { kind: 'delete'; paths: string[] }
  | { kind: 'compress'; paths: string[]; destination: string }
  | { kind: 'extract'; path: string };

/** The places sidebar snapshot returned by `files.places`. */
export interface Places {
  pins: Pin[];
  drives: DriveInfo[];
}

/** A user-pinned "open with" action. Mirrors the Rust `PinnedAction` struct. */
export interface PinnedAction {
  desktop_id: string;
  label: string;
}

/** Persisted file-explorer preferences. Wire fields are snake_case to match the Rust DTO. */
export interface FilePreferences {
  show_hidden: boolean;
  pinned_actions: PinnedAction[];
}

/** The kind of preview a path resolves to. Mirrors the Rust `PreviewKind` enum. */
export type PreviewKind = 'image' | 'text' | 'none';

/** A preview payload returned by `files.preview`. Mirrors the Rust `PreviewPayload` struct. */
export interface PreviewPayload {
  kind: PreviewKind;
  data: string;
}

/**
 * An event published on the `files.event` channel. Discriminated union on
 * `event`, mirroring the Rust `FilesEvent` enum.
 */
export type FilesEvent =
  | { event: 'changed'; path: string }
  | { event: 'size'; path: string; bytes: number; complete: boolean }
  | { event: 'operation_complete'; operation: FileOperation }
  | { event: 'operation_failed'; message: string };
