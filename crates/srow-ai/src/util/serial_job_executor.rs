use std::future::Future;
use std::pin::Pin;
use tokio::sync::{mpsc, oneshot};

type Job = Pin<Box<dyn Future<Output = ()> + Send>>;

/// Executes async jobs one at a time, in order.
/// Spawned on a specific tokio runtime handle (not the current thread).
pub struct SerialJobExecutor {
    tx: mpsc::UnboundedSender<Job>,
}

impl SerialJobExecutor {
    /// Create a new executor. The consumer loop is spawned on the given runtime.
    pub fn new(handle: &tokio::runtime::Handle) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<Job>();
        handle.spawn(async move {
            while let Some(job) = rx.recv().await {
                job.await;
            }
        });
        Self { tx }
    }

    /// Submit a job and wait for it to complete.
    pub async fn run<F>(&self, job: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let (done_tx, done_rx) = oneshot::channel();
        let _ = self.tx.send(Box::pin(async move {
            job.await;
            let _ = done_tx.send(());
        }));
        let _ = done_rx.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn test_jobs_execute_sequentially() {
        let handle = tokio::runtime::Handle::current();
        let executor = SerialJobExecutor::new(&handle);
        let log = Arc::new(Mutex::new(Vec::new()));

        let log1 = log.clone();
        executor
            .run(async move {
                log1.lock().await.push("a_start");
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                log1.lock().await.push("a_end");
            })
            .await;

        let log2 = log.clone();
        executor
            .run(async move {
                log2.lock().await.push("b_start");
                log2.lock().await.push("b_end");
            })
            .await;

        let result = log.lock().await;
        assert_eq!(*result, vec!["a_start", "a_end", "b_start", "b_end"]);
    }
}
