// Multi-Agent Project In-Depth Research System
// A (Macro, C1) = SystemContextResearcher: What is this project, functional overview, upstream/downstream = README.md + structure + code_insights-top50

// B (Meso, C2): DomainModulesDetector: What domain modules exist from a high-level design perspective, what do they do = A + structure + code_insights-top50 + relationship-top50
// C (Meso, C2): ArchitectureResearcher: What is the architecture design = A + B
// D (Meso, C2): WorkflowResearcher: What are the workflows = A + B

// E (Micro, C3): KeyModulesInsight: Detailed technical solutions for each module = related E + related code_insights
// F (Micro, C3, C4): BoundariesInsight: Categorize by focused Purpose, extract explanations of code that belongs to boundary types.

pub mod agents;
pub mod orchestrator;
pub mod types;
pub mod memory;
