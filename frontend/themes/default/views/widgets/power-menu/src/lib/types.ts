/**
 * State shapes shared by the power-menu view's components. Mirrors the
 * Rust DTOs in `crates/domain/src/bar_state.rs::SystemPowerState` and
 * the schedule queue's `ScheduledJobSummary`.
 */

export interface SystemPowerState {
    can_shutdown: boolean;
    can_restart: boolean;
    can_suspend: boolean;
    can_hibernate: boolean;
    can_lock: boolean;
}

export interface ScheduledJob {
    id: string;
    /**
     * Default serde serialization of `std::time::SystemTime`. We use
     * the duration-since-epoch to compute "in N minutes" relative to
     * the current wall clock.
     */
    fires_at: { secs_since_epoch: number; nanos_since_epoch: number };
    label: string;
}

export type PowerCommand = 'shutdown' | 'restart' | 'suspend' | 'hibernate' | 'lock';
