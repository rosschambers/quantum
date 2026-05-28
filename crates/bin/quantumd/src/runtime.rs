use std::thread::JoinHandle;
use tokio::runtime::{Handle, Runtime};
use tokio::sync::oneshot;

pub struct WorkerRuntime {
    pub handle: Handle,
    shutdown_tx: Option<oneshot::Sender<()>>,
    join_handle: Option<JoinHandle<()>>,
}

impl WorkerRuntime {
    pub fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(join) = self.join_handle.take() {
            let _ = join.join();
        }
    }
}

impl Drop for WorkerRuntime {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(join) = self.join_handle.take() {
            let _ = join.join();
        }
    }
}

pub fn spawn_worker(runtime: Runtime) -> std::io::Result<WorkerRuntime> {
    let handle = runtime.handle().clone();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let join_handle = std::thread::Builder::new()
        .name("quantum-tokio".to_string())
        .spawn(move || {
            runtime.block_on(async move {
                let _ = shutdown_rx.await;
            });
        })?;
    Ok(WorkerRuntime {
        handle,
        shutdown_tx: Some(shutdown_tx),
        join_handle: Some(join_handle),
    })
}
