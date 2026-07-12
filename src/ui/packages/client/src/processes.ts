// Hand-written TypeScript DTOs mirroring the Rust process task-manager types
// from `quantum-domain` (`src/domain/src/processes.rs`, serde output). There is
// no codegen; keep these shapes and their snake_case field names in exact
// lockstep with the Rust structs and enums. This file is DTO-only, matching
// `files.ts` and `timer.ts` — no client wrapper lives here.
//
// The daemon exposes these process methods (documented here, not wrapped):
//   processes.watch    start streaming process snapshots
//   processes.unwatch  stop streaming process snapshots
//   processes.kill     signal a process subtree
// Live snapshots are published on the `processes.event` channel as the
// serialized JSON of a `ProcessSnapshot`.

/** Machine-wide resource usage sampled alongside a process snapshot. */
export interface GlobalStats {
  cpu_percent: number;
  mem_used_bytes: number;
  mem_total_bytes: number;
  net_rx_bytes_per_second: number;
  net_tx_bytes_per_second: number;
}

/** The window a process owns, when it has one. Mirrors the Rust `WindowInfo` struct. */
export interface WindowInfo {
  class: string;
  title: string;
}

/**
 * A node in the process forest: one process plus the subtree it roots. Carries
 * both self usage and the aggregate usage of itself and all descendants. The
 * `window` field is omitted when the process owns no window (Rust
 * `#[serde(skip_serializing_if = "Option::is_none")]`).
 */
export interface ProcessNode {
  pid: number;
  name: string;
  cpu_percent: number;
  mem_bytes: number;
  aggregate_cpu_percent: number;
  aggregate_mem_bytes: number;
  window?: WindowInfo;
  protected: boolean;
  children: ProcessNode[];
}

/**
 * A full sampling of the machine's processes split into windowed applications
 * and background processes, alongside global resource usage.
 */
export interface ProcessSnapshot {
  global: GlobalStats;
  apps: ProcessNode[];
  background: ProcessNode[];
}

/**
 * Which signal to deliver when terminating a process subtree. Mirrors the Rust
 * `KillSignal` enum, which serializes as a lowercase name.
 */
export type KillSignal = 'term' | 'kill';

/** The channel that streams serialized `ProcessSnapshot` payloads. */
export const PROCESSES_EVENT_CHANNEL = 'processes.event';

/** Dispatcher method: start streaming process snapshots. */
export const PROCESSES_WATCH = 'processes.watch';

/** Dispatcher method: stop streaming process snapshots. */
export const PROCESSES_UNWATCH = 'processes.unwatch';

/** Dispatcher method: signal a process subtree with a `KillSignal`. */
export const PROCESSES_KILL = 'processes.kill';
