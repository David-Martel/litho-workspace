use std::future::Future;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

pub async fn do_parallel_with_limit<F, T>(futures: Vec<F>, mut max_concurrent: usize) -> Vec<T>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    if max_concurrent == 0 {
        max_concurrent = 1;
    }
    let total = futures.len();
    let semaphore = Arc::new(Semaphore::new(max_concurrent));
    let mut join_set = JoinSet::new();

    for (index, fut) in futures.into_iter().enumerate() {
        let permit = Arc::clone(&semaphore);
        join_set.spawn(async move {
            let _permit = permit
                .acquire_owned()
                .await
                .expect("parallel task semaphore was closed unexpectedly");
            (index, fut.await)
        });
    }

    let mut results = std::iter::repeat_with(|| None)
        .take(total)
        .collect::<Vec<Option<T>>>();

    while let Some(join_result) = join_set.join_next().await {
        match join_result {
            Ok((index, value)) => {
                results[index] = Some(value);
            }
            Err(err) => {
                join_set.abort_all();
                while join_set.join_next().await.is_some() {}
                if err.is_panic() {
                    std::panic::resume_unwind(err.into_panic());
                }
                panic!("parallel task failed: {err}");
            }
        }
    }

    results
        .into_iter()
        .map(|value| value.expect("missing parallel task result"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::do_parallel_with_limit;
    use std::future::Future;
    use std::pin::Pin;

    #[tokio::test]
    async fn preserves_input_order_even_when_completion_order_differs() {
        let futures: Vec<Pin<Box<dyn Future<Output = &'static str> + Send>>> = vec![
            Box::pin(async {
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                "first"
            }),
            Box::pin(async {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                "second"
            }),
            Box::pin(async {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                "third"
            }),
        ];

        let results = do_parallel_with_limit(futures, 2).await;
        assert_eq!(results, vec!["first", "second", "third"]);
    }
}
