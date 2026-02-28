use crate::generator::context::GeneratorContext;
use serde_json::Value;

pub struct MemoryScope;

impl MemoryScope {
    pub const STUDIES_RESEARCH: &'static str = "studies_research";
}

pub trait MemoryRetriever {
    fn store_research(
        &self,
        agent_type: &str,
        result: Value,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send;

    fn get_research(
        &self,
        agent_type: &str,
    ) -> impl std::future::Future<Output = Option<Value>> + Send;
}

impl MemoryRetriever for GeneratorContext {
    /// Store research results
    async fn store_research(&self, agent_type: &str, result: Value) -> anyhow::Result<()> {
        self.store_to_memory(MemoryScope::STUDIES_RESEARCH, agent_type, result)
            .await
    }

    /// Get research results
    async fn get_research(&self, agent_type: &str) -> Option<Value> {
        self.get_from_memory(MemoryScope::STUDIES_RESEARCH, agent_type)
            .await
    }
}
