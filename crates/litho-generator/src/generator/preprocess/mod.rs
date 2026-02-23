use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::time::Instant;

use crate::generator::preprocess::extractors::original_document_extractor;
use crate::generator::preprocess::memory::{MemoryScope, ScopedKeys};
use crate::types::original_document::OriginalDocument;
use crate::{
    generator::{
        context::GeneratorContext,
        preprocess::{
            agents::{code_analyze::CodeAnalyze, relationships_analyze::RelationshipsAnalyze},
            extractors::structure_extractor::StructureExtractor,
        },
        types::Generator,
    },
    types::{
        code::CodeInsight, code_releationship::RelationshipAnalysis,
        project_structure::ProjectStructure,
    },
};

pub mod agents;
pub mod extractors;
pub mod memory;

/// Preprocessing result
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PreprocessingResult {
    // Original document materials extracted from the project, may not be accurate and is for reference only
    pub original_document: OriginalDocument,
    // Project structure information
    pub project_structure: ProjectStructure,
    // Intelligent insights of core code
    pub core_code_insights: Vec<CodeInsight>,
    // Dependencies between code
    pub relationships: RelationshipAnalysis,
    pub processing_time: f64,
}

pub struct PreProcessAgent {}

impl PreProcessAgent {
    pub fn new() -> Self {
        Self {}
    }
}

impl Generator<PreprocessingResult> for PreProcessAgent {
    async fn execute(&self, context: GeneratorContext) -> Result<PreprocessingResult> {
        let start_time = Instant::now();

        let structure_extractor = StructureExtractor::new(context.clone());
        let config = &context.config;

        println!("🔍 Starting project preprocessing phase...");

        // 1. Extract project original document materials
        println!("📁 Extracting project original document materials...");
        let original_document = original_document_extractor::extract(&context).await?;

        // 2. Extract project structure
        println!("📁 Extracting project structure...");
        let project_structure = structure_extractor
            .extract_structure(&config.project_path)
            .await?;

        println!(
            "   🔭 Discovered {} files, {} directories",
            project_structure.total_files, project_structure.total_directories
        );

        // 3. Identify core components
        println!("🎯 Identifying main source code files...");
        let important_codes = structure_extractor
            .identify_core_codes(&project_structure)
            .await?;

        println!("   Identified {} main source code files", important_codes.len());

        // 4. Analyze core components using AI
        println!("🤖 Analyzing core files using AI...");
        let code_analyze = CodeAnalyze::new();
        let core_code_insights = code_analyze
            .execute(&context, &important_codes, &project_structure)
            .await?;

        // 5. Analyze component relationships
        println!("🔗 Analyzing component relationships...");
        let relationships_analyze = RelationshipsAnalyze::new();
        let relationships = relationships_analyze
            .execute(&context, &core_code_insights, &project_structure)
            .await?;

        let processing_time = start_time.elapsed().as_secs_f64();

        println!("✅ Project preprocessing completed, took {:.2} seconds", processing_time);

        // 6. Store preprocessing results to Memory
        context
            .store_to_memory(
                MemoryScope::PREPROCESS,
                ScopedKeys::PROJECT_STRUCTURE,
                &project_structure,
            )
            .await?;
        context
            .store_to_memory(
                MemoryScope::PREPROCESS,
                ScopedKeys::CODE_INSIGHTS,
                &core_code_insights,
            )
            .await?;
        context
            .store_to_memory(
                MemoryScope::PREPROCESS,
                ScopedKeys::RELATIONSHIPS,
                &relationships,
            )
            .await?;
        context
            .store_to_memory(
                MemoryScope::PREPROCESS,
                ScopedKeys::ORIGINAL_DOCUMENT,
                &original_document,
            )
            .await?;

        Ok(PreprocessingResult {
            original_document,
            project_structure,
            core_code_insights,
            relationships,
            processing_time,
        })
    }
}
