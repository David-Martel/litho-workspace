use anyhow::Result;

use crate::generator::context::GeneratorContext;

pub trait Generator<T> {
    async fn execute(&self, context: GeneratorContext) -> Result<T>;
}
