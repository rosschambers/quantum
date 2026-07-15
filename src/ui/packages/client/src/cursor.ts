// Hand-written TypeScript DTO mirroring the Rust `CursorPosition` type from
// `quantum-domain` (`src/domain/src/cursor.rs`, serde output). There is no
// codegen; keep this shape and its field names in exact lockstep with the Rust
// struct. This file is DTO-only, matching `processes.ts` and `files.ts` — no
// client wrapper lives here.
//
// The daemon exposes these cursor methods (documented here, not wrapped):
//   cursor.watch    start streaming cursor positions
//   cursor.unwatch  stop streaming cursor positions
// Live positions are published on the `cursor.event` channel as the serialized
// JSON of a `CursorPosition`.

/** A pointer position in compositor-global (layout) coordinates. */
export interface CursorPosition {
  x: number;
  y: number;
}

/** The channel that streams serialized `CursorPosition` payloads. */
export const CURSOR_EVENT_CHANNEL = 'cursor.event';

/** Dispatcher method: start streaming cursor positions. */
export const CURSOR_WATCH = 'cursor.watch';

/** Dispatcher method: stop streaming cursor positions. */
export const CURSOR_UNWATCH = 'cursor.unwatch';
