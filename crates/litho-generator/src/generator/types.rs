use anyhow::Result;

use crate::generator::context::GeneratorContext;

pub trait Generator<T> {
    fn execute(
        &self,
        context: GeneratorContext,
    ) -> impl std::future::Future<Output = Result<T>> + Send;
}
