//! Integration test that samples the LIVE `/proc` filesystem twice. Unlike the
//! pure parser unit tests, this exercises the real directory walk and counter
//! retention against the running kernel. It asserts only invariants that hold on
//! any Linux host: the test's own process is present, total memory is positive,
//! and sampling never panics.

use std::time::Duration;

use quantum_processes::ProcfsSampler;

#[tokio::test]
async fn samples_live_proc_twice_and_sees_own_process() {
    let mut sampler = ProcfsSampler::new();

    // First sample establishes the baseline (0% CPU, zero network rates).
    let (first_processes, _first_global) = sampler
        .sample()
        .await
        .expect("first live /proc sample succeeds");
    assert!(
        !first_processes.is_empty(),
        "a running system always has processes"
    );

    // A short delay so the second sample has a non-zero interval over which the
    // kernel counters can advance.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let (processes, global) = sampler
        .sample()
        .await
        .expect("second live /proc sample succeeds");

    let own_pid = std::process::id() as i32;
    assert!(
        processes.iter().any(|process| process.pid == own_pid),
        "the test's own pid {own_pid} should appear in the sample"
    );
    assert!(
        global.mem_total_bytes > 0,
        "total memory should be positive on a live host"
    );
}
