import { describe, it, expect } from 'vitest';
import type { KillSignal, ProcessSnapshot } from './processes';
import {
  PROCESSES_EVENT_CHANNEL,
  PROCESSES_WATCH,
  PROCESSES_UNWATCH,
  PROCESSES_KILL,
} from './processes';

describe('ProcessSnapshot', () => {
  // A representative serialized snapshot mirroring the Rust serde output in
  // `src/domain/src/processes.rs`. The application root carries a `window`; the
  // nested child and the background node omit `window` entirely (Rust skips the
  // field when the process owns no window).
  const raw =
    '{"global":{"cpu_percent":12.5,"mem_used_bytes":4000000000,' +
    '"mem_total_bytes":16000000000,"net_rx_bytes_per_second":1024,' +
    '"net_tx_bytes_per_second":512},"apps":[{"pid":100,"name":"firefox",' +
    '"cpu_percent":5.0,"mem_bytes":500000000,"aggregate_cpu_percent":7.0,' +
    '"aggregate_mem_bytes":600000000,"window":{"class":"firefox",' +
    '"title":"Mozilla Firefox"},"protected":false,"children":[{"pid":200,' +
    '"name":"firefox-tab","cpu_percent":2.0,"mem_bytes":100000000,' +
    '"aggregate_cpu_percent":2.0,"aggregate_mem_bytes":100000000,' +
    '"protected":false,"children":[]}]}],"background":[{"pid":300,' +
    '"name":"quantumd","cpu_percent":1.0,"mem_bytes":50000000,' +
    '"aggregate_cpu_percent":1.0,"aggregate_mem_bytes":50000000,' +
    '"protected":true,"children":[]}]}';

  it('parses a serialized snapshot and satisfies the ProcessSnapshot type', () => {
    const snapshot: ProcessSnapshot = JSON.parse(raw);

    expect(snapshot.global.cpu_percent).toBe(12.5);
    expect(snapshot.global.mem_total_bytes).toBe(16000000000);

    const root = snapshot.apps[0];
    expect(root.pid).toBe(100);
    expect(root.window?.class).toBe('firefox');
    expect(root.protected).toBe(false);

    // Nested child carries no window key.
    const child = root.children[0];
    expect(child.pid).toBe(200);
    expect(child.window).toBeUndefined();
    expect(child.children).toEqual([]);

    const background = snapshot.background[0];
    expect(background.protected).toBe(true);
    expect(background.window).toBeUndefined();
  });
});

describe('KillSignal union', () => {
  it('accepts term and kill', () => {
    const term: KillSignal = 'term';
    const kill: KillSignal = 'kill';
    expect(term).toBe('term');
    expect(kill).toBe('kill');
  });
});

describe('processes channel and method constants', () => {
  it('match the dispatcher names', () => {
    expect(PROCESSES_EVENT_CHANNEL).toBe('processes.event');
    expect(PROCESSES_WATCH).toBe('processes.watch');
    expect(PROCESSES_UNWATCH).toBe('processes.unwatch');
    expect(PROCESSES_KILL).toBe('processes.kill');
  });
});
