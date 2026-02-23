use futures::future::join_all;
use std::future::Future;
use std::sync::Arc;
use tokio::sync::Semaphore;

pub async fn do_parallel_with_limit<F, T>(futures: Vec<F>, mut max_concurrent: usize) -> Vec<T>
where
    F: Future<Output = T> + Send + 'static,
{
    if max_concurrent == 0 {
        max_concurrent = 1;
    }
    let semaphore = Arc::new(Semaphore::new(max_concurrent));

    let controlled_futures: Vec<_> = futures
        .into_iter()
        .map(|fut| {
            let permit = Arc::clone(&semaphore);
            async move {
                let _permit = permit.acquire().await.unwrap();
                fut.await
            }
        })
        .collect();

    join_all(controlled_futures).await
}
