use crate::generator::research::memory::MemoryScope;
use crate::generator::research::types::{AgentType, WorkflowReport};
use crate::generator::step_forward_agent::{
    AgentDataConfig, DataSource, FormatterConfig, LLMCallMode, PromptTemplate, StepForwardAgent,
};

#[derive(Default)]
pub struct WorkflowResearcher;

impl StepForwardAgent for WorkflowResearcher {
    type Output = WorkflowReport;

    fn agent_type(&self) -> String {
        AgentType::WorkflowResearcher.to_string()
    }

    fn agent_type_enum(&self) -> Option<AgentType> {
        Some(AgentType::WorkflowResearcher)
    }

    fn memory_scope_key(&self) -> String {
        MemoryScope::STUDIES_RESEARCH.to_string()
    }

    fn data_config(&self) -> AgentDataConfig {
        AgentDataConfig {
            required_sources: vec![
                DataSource::ResearchResult(AgentType::SystemContextResearcher.to_string()),
                DataSource::ResearchResult(AgentType::DomainModulesDetector.to_string()),
                DataSource::CODE_INSIGHTS,
            ],
            // Use workflow docs for business process analysis
            optional_sources: vec![DataSource::knowledge_categories(vec![
                "workflow",
                "architecture",
            ])],
        }
    }

    fn prompt_template(&self) -> PromptTemplate {
        PromptTemplate {
            system_prompt: r#"Analyze the project's core functional workflows, focusing from a functional perspective without being limited to excessive technical details.

GROUNDING RULES (critical for accuracy):
- Only describe workflows that are ACTUALLY IMPLEMENTED as code paths. Evidence must include specific function names, file paths, or entry points.
- Do NOT invent workflows for utility modules, configuration files, or MCP servers that merely provide data.
- If the project is a pipeline (e.g., build system, data pipeline, CV generator), describe the actual pipeline stages as they appear in the main entry point.
- If a code file is a standalone script or utility, it is NOT a workflow — it is a tool.
- Confidence must be below 3 for any workflow inferred only from file/directory names without tracing actual code execution paths.

You may have access to existing product description, requirements and architecture documentation from external sources.
If available:
- Cross-reference code workflows with documented business processes
- Use established process terminology and flow descriptions
- Validate implementation against documented process requirements
- Identify any gaps between documented workflows and actual implementation
- Incorporate business context and rationale from the documentation"#.to_string(),
            opening_instruction: "The following research reports are provided for analyzing the system's main workflows".to_string(),
            closing_instruction: r#"Please analyze the system's core workflows based on the research materials.

If external documentation is provided:
- Validate code workflows against documented business processes
- Note any discrepancies or missing steps
- Use consistent process terminology

IMPORTANT: Do NOT fabricate workflows that are not evidenced in the code. Write "Insufficient evidence" rather than guessing."#.to_string(),
            llm_call_mode: LLMCallMode::Extract,
            formatter_config: FormatterConfig::default(),
        }
    }
}
