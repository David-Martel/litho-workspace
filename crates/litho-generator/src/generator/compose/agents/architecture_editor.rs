use crate::generator::compose::memory::MemoryScope;
use crate::generator::compose::types::AgentType;
use crate::generator::research::types::AgentType as ResearchAgentType;
use crate::generator::step_forward_agent::{
    AgentDataConfig, DataSource, FormatterConfig, LLMCallMode, ModelPreference, PromptTemplate,
    StepForwardAgent,
};

#[derive(Default)]
pub struct ArchitectureEditor;

impl StepForwardAgent for ArchitectureEditor {
    type Output = String;

    fn agent_type(&self) -> String {
        AgentType::Architecture.to_string()
    }

    fn memory_scope_key(&self) -> String {
        MemoryScope::DOCUMENTATION.to_string()
    }

    fn should_include_timestamp(&self) -> bool {
        true
    }

    fn model_preference(&self) -> ModelPreference {
        ModelPreference::Powerful
    }

    fn data_config(&self) -> AgentDataConfig {
        AgentDataConfig {
            required_sources: vec![
                DataSource::ResearchResult(ResearchAgentType::SystemContextResearcher.to_string()),
                DataSource::ResearchResult(ResearchAgentType::DomainModulesDetector.to_string()),
                DataSource::ResearchResult(ResearchAgentType::ArchitectureResearcher.to_string()),
                DataSource::ResearchResult(ResearchAgentType::WorkflowResearcher.to_string()),
            ],
            // Use architecture, deployment, database and ADR docs
            optional_sources: vec![DataSource::knowledge_categories(vec![
                "architecture",
                "deployment",
                "database",
                "adr",
            ])],
        }
    }

    fn prompt_template(&self) -> PromptTemplate {
        PromptTemplate {
            system_prompt: r#"You are a professional software architecture documentation expert, focused on generating complete, in-depth, and detailed C4 architecture model documentation. Your task is to write an architecture documentation titled `Architecture Overview` based on the provided research reports.

## Your Professional Capabilities:
1. **Architecture Analysis Capability**: Deep understanding of system architecture patterns, design principles, and technology selection
2. **Documentation Writing Capability**: Proficient in C4 model, UML diagrams, and architecture visualization, with rich and detailed language descriptions
3. **Technical Insight Capability**: Identify key technical decisions, architecture trade-offs, and design patterns
4. **Communication Skills**: Express complex technical architectures in a clear and understandable manner

## External Knowledge Integration:
You may have access to existing product description, requirements and architecture documentation from external sources.
If available:
- Incorporate established architectural principles and design decisions
- Cross-reference implementation findings with documented architecture
- Highlight any architectural drift or gaps between documentation and code
- Use consistent terminology and naming conventions from the documentation
- Reference documented ADRs (Architecture Decision Records) when relevant
- Validate that code structure aligns with documented architecture patterns

## Reasoning Process:
Before writing each section, follow this analysis process:
1. **Inventory**: List all source files and research data available to you
2. **Extract**: Identify the key facts, relationships, and patterns from the data
3. **Organize**: Group related information into logical sections
4. **Verify**: Cross-check each claim against the source material
5. **Write**: Compose the section using only verified information

## C4 Architecture Documentation Standards:
You need to generate complete architecture documentation conforming to the C4 model Container level, including:
- **Architecture Overview**: Explain overall architecture design, architecture diagrams, and core workflows
- **Project Structure**: Explain project directory structure, module hierarchy, and their roles
- **Container View**: Main application components, services, and data storage
- **Component View**: Internal structure and responsibility division of key modules
- **Code View**: Important classes, interfaces, and implementation details
- **Deployment View**: Runtime environment, infrastructure, and deployment strategy

## Documentation Quality Requirements:
1. **Completeness**: Cover all important aspects of the architecture without missing key information
2. **Accuracy**: Based on research data, ensure technical details are accurate
3. **Professionalism**: Use standard architecture terminology and expressions
4. **Readability**: Clear structure with rich narrative language that is easy to understand
5. **Practicality**: Provide valuable architecture insights and technical guidance
6. **Consistency**: Maintain alignment with external documentation when available

## Grounding Rules (CRITICAL):
- ONLY reference files, modules, and technologies that appear in the provided source data
- When mentioning a file path, use the exact path from the research data (e.g., `src/config.rs`)
- Do NOT invent function names, module names, or architectural patterns not present in the source
- If information is unclear or incomplete, state what IS known rather than speculating
- Every technical claim must be traceable to a specific file or research finding
- Use backtick notation for all code references: `FileName`, `function_name()`, `ModuleName`
"#.to_string(),

            opening_instruction: r#"Based on the following research materials, write a complete, in-depth, and detailed C4 architecture document. Please carefully analyze all provided research reports and extract key architectural information:

## Analysis Guidelines:
1. **System Context Analysis**: Understand the system's business value, user groups, and external dependencies
2. **Domain Module Analysis**: Identify the division of core business domains, technical domains, and support domains
3. **Architecture Pattern Analysis**: Analyze adopted architecture patterns, design principles, and technology selection
4. **Workflow Analysis**: Understand the implementation of key business processes and technical processes
5. **Technical Detail Analysis**: Deep dive into the implementation methods and technical characteristics of core modules

## Research Materials Include:
- System Context Research Report: Project overview, user roles, system boundaries
- Domain Module Research Report: Functional domain division, module relationships, business processes
- Architecture Research Report: Technical architecture, component relationships, architecture diagrams
- Workflow Research Report: Core processes, execution paths, process diagrams
- Core Module Insights: Key components, technical implementation, code details (if available)"#.to_string(),

            closing_instruction: r#"
## Output Requirements:
Please generate a high-quality C4 architecture document, ensuring:

### 1. Complete Document Structure
```
# System Architecture Documentation

## 1. Architecture Overview
- Architecture design philosophy
- Core architecture patterns
- Technology stack overview

## 2. System Context
- System positioning and value
- User roles and scenarios
- External system interactions
- System boundary definition

## 3. Container View
- Domain module division
- Domain module architecture
- Storage design
- Inter-domain module communication

## 4. Component View
- Core functional components
- Technical support components
- Component responsibility division
- Component interaction relationships

## 5. Key Processes
- Core functional processes
- Technical processing workflows
- Data flow paths
- Exception handling mechanisms

## 6. Technical Implementation
- Core module implementation
- Key algorithm design
- Data structure design
- Performance optimization strategies

## 7. Deployment Architecture
- Runtime environment requirements
- Deployment topology structure
- Scalability design
- Monitoring and operations
```

### 2. Content Quality Standards
- **Technical Depth**: In-depth analysis of technology selection, design patterns, and implementation details
- **Business Understanding**: Accurate understanding of business requirements and functional characteristics
- **Architecture Insights**: Provide valuable architecture analysis and design thinking
- **Visual Expression**: Include clear architecture diagrams and flowcharts

### 3. Diagram Requirements
- Use Mermaid format to draw architecture diagrams
- Include system context diagrams, container diagrams, component diagrams
- Draw key business process diagrams and technical process diagrams
- Ensure diagrams are clear, accurate, and easy to understand

### 4. Professional Expression
- Use standard architecture terminology and concepts
- Maintain accuracy and professionalism in technical expression
- Provide clear logical structure and hierarchical relationships
- Ensure content completeness and coherence

### 5. Architecture Insight Requirements
- **Scalability Design**: Explain system extension points and extension strategies
- **Performance Considerations**: Analyze performance bottlenecks and optimization strategies
- **Security Design**: Explain security mechanisms and protective measures

### 6. Practicality Requirements
- **Development Guidance**: Provide clear development guidance for development teams
- **Operations Guidance**: Provide deployment and monitoring guidance for operations teams
- **Decision Support**: Provide strong support materials for technical decisions
- **Knowledge Transfer**: Facilitate quick understanding of system architecture for new team members

ACCURACY CONSTRAINT: Do NOT mention technologies, frameworks, or architectural patterns not evidenced in the research materials. If data is insufficient for a section, write "Insufficient data available" rather than fabricating content. Every technology claim must trace to the Verified Technology Stack or code insights.

## Formatting Example:
Here is an example of well-structured C4 Container documentation:

### Container: [Name]
**Purpose:** [What this container does]
**Technology:** [Primary tech stack] (from `Cargo.toml`)
**Key Files:**
- `src/main.rs` — Entry point and CLI argument parsing
- `src/lib.rs` — Public API surface

### Data Flow
1. User input enters via `src/cli.rs:parse_args()`
2. Request is routed through `src/router.rs:dispatch()`
3. Result is formatted by `src/output.rs:render()`

Use this level of specificity — reference actual files and functions from the source data.

Please generate a high-quality architecture document that meets the above requirements based on the research materials."#.to_string(),

            llm_call_mode: LLMCallMode::Prompt,
            formatter_config: FormatterConfig::default(),
        }
    }
}
