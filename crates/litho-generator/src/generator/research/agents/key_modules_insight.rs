use crate::generator::preprocess::memory::{MemoryScope, ScopedKeys};
use crate::generator::research::memory::MemoryRetriever;
use crate::generator::research::types::{
    AgentType, DomainModule, DomainModulesReport, KeyModuleReport, SubModule,
};
use crate::generator::{
    agent_executor::{AgentExecuteParams, extract},
    context::GeneratorContext,
    step_forward_agent::{
        AgentDataConfig, DataSource, FormatterConfig, LLMCallMode, ModelPreference, PromptTemplate,
        StepForwardAgent,
    },
};
use crate::types::code::CodeInsight;
use crate::utils::threads::do_parallel_with_limit;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use std::collections::HashSet;

// Research materials for domain modules
#[derive(Default, Clone)]
pub struct KeyModulesInsight;

#[async_trait]
impl StepForwardAgent for KeyModulesInsight {
    type Output = Vec<KeyModuleReport>;

    fn agent_type(&self) -> String {
        AgentType::KeyModulesInsight.to_string()
    }

    fn agent_type_enum(&self) -> Option<AgentType> {
        Some(AgentType::KeyModulesInsight)
    }

    fn memory_scope_key(&self) -> String {
        crate::generator::research::memory::MemoryScope::STUDIES_RESEARCH.to_string()
    }

    fn model_preference(&self) -> ModelPreference {
        ModelPreference::Efficient
    }

    fn data_config(&self) -> AgentDataConfig {
        AgentDataConfig {
            required_sources: vec![
                DataSource::ResearchResult(AgentType::SystemContextResearcher.to_string()),
                DataSource::ResearchResult(AgentType::DomainModulesDetector.to_string()),
            ],
            // Use architecture and database docs for module insights
            optional_sources: vec![DataSource::knowledge_categories(vec![
                "architecture",
                "database",
            ])],
        }
    }

    fn prompt_template(&self) -> PromptTemplate {
        PromptTemplate {
            system_prompt: r#"You are a software development expert. Based on the information provided by the user, investigate the technical details of core modules.

For each module, provide a structured analysis covering:
1. PURPOSE: What problem does this module solve? What is its role in the system?
2. MECHANISM: How does it work? Describe the key algorithms, data transformations, or control flow.
3. DATA FLOW: What are the inputs and outputs? How does data move through this module?
4. DEPENDENCIES: What does this module depend on? What depends on it?
5. ERROR HANDLING: How does it handle failures, edge cases, or invalid input?

GROUNDING RULES:
- Every claim must reference a specific file path or function name from the code insights.
- Do NOT describe capabilities that are not present in the source code.
- If a module's purpose is unclear from the code, say "Purpose unclear from available code" rather than guessing.
- Use concrete code references (file paths, function names, type names) over abstract descriptions.

You may have access to existing product description, requirements and architecture documentation from external sources.
If available:
- Reference documented component responsibilities and interfaces
- Validate implementation against documented design patterns
- Use established terminology for components and modules"#
                .to_string(),
            opening_instruction: "Based on the following project information and research materials, analyze the core modules:".to_string(),
            closing_instruction: "Do NOT mention technologies not listed in the research materials. Write 'Insufficient data' rather than fabricating content.".to_string(),
            llm_call_mode: LLMCallMode::Extract,
            formatter_config: FormatterConfig::default(),
        }
    }

    // Override execute method to implement multi-domain analysis
    async fn execute(&self, context: &GeneratorContext) -> Result<Self::Output> {
        let reports = self.execute_multi_domain_analysis(context).await?;
        let value = serde_json::to_value(&reports)?;

        context
            .store_to_memory(&self.memory_scope_key(), &self.agent_type(), value.clone())
            .await?;

        Ok(reports)
    }
}

impl KeyModulesInsight {
    // Multi-domain analysis main logic
    async fn execute_multi_domain_analysis(
        &self,
        context: &GeneratorContext,
    ) -> Result<Vec<KeyModuleReport>> {
        println!("🔍 Starting multi-domain module analysis...");
        let mut reports = vec![];
        let max_parallels = context.config.llm.max_parallels;

        // 1. Get domain module data
        let domain_modules = self.get_domain_modules(context).await?;

        if domain_modules.is_empty() {
            return Err(anyhow!("No domain module data found"));
        }

        let domain_names: Vec<String> = domain_modules.iter().map(|d| d.name.clone()).collect();
        println!(
            "📋 Discovered {} domain modules: {}",
            domain_modules.len(),
            domain_names.join(", ")
        );

        // 2. Perform concurrent analysis for each domain module
        println!(
            "🚀 Starting concurrent analysis, max parallelism: {}",
            max_parallels
        );

        // Create concurrent tasks
        let analysis_futures: Vec<_> = domain_modules
            .iter()
            .map(|domain| {
                let domain_clone = domain.clone();
                let context_clone = context.clone();
                Box::pin(async move {
                    let key_modules_insight = KeyModulesInsight;
                    let result = key_modules_insight
                        .analyze_single_domain(&domain_clone, &context_clone)
                        .await;
                    (domain_clone.name.clone(), result)
                })
            })
            .collect();

        // Use do_parallel_with_limit for concurrency control
        let analysis_results = do_parallel_with_limit(analysis_futures, max_parallels).await;

        // Process analysis results
        let mut successful_analyses = 0;
        for (domain_name, result) in analysis_results {
            match result {
                Ok(report) => {
                    // Store results for each domain
                    let storage_key = format!("{}_{}", self.agent_type(), domain_name);
                    context
                        .store_research(&storage_key, serde_json::to_value(&report)?)
                        .await?;
                    successful_analyses += 1;
                    reports.push(report);
                    println!(
                        "✅ Domain module analysis: {} completed and stored",
                        domain_name
                    );
                }
                Err(e) => {
                    let msg = context.config.target_language.msg_domain_analysis_failed();
                    println!(
                        "{}",
                        msg.replace("{}", &domain_name)
                            .replace("{}", &e.to_string())
                    );
                    // Continue processing other domains without interrupting the entire flow
                }
            }
        }

        if successful_analyses == 0 {
            return Err(anyhow!("All domain analyses failed"));
        }

        Ok(reports)
    }
}

impl KeyModulesInsight {
    // Get domain module data.
    //
    // Returns an empty Vec when the DomainModulesDetector result is absent
    // instead of propagating an error.  The caller (`execute_multi_domain_analysis`)
    // already handles an empty list by returning an appropriate error message,
    // so crashing here would just produce a worse diagnostic.
    async fn get_domain_modules(&self, context: &GeneratorContext) -> Result<Vec<DomainModule>> {
        let Some(domain_report) = context
            .get_research(&AgentType::DomainModulesDetector.to_string())
            .await
        else {
            eprintln!(
                "Warning: DomainModulesDetector result is not available; \
                 skipping domain-level module analysis"
            );
            return Ok(Vec::new());
        };

        let domain_modules_report: DomainModulesReport = serde_json::from_value(domain_report)?;
        Ok(domain_modules_report.domain_modules)
    }

    // Filter code insights related to the domain
    async fn filter_code_insights_for_domain(
        &self,
        domain: &DomainModule,
        context: &GeneratorContext,
    ) -> Result<Vec<CodeInsight>> {
        let all_insights = context
            .get_from_memory::<Vec<CodeInsight>>(MemoryScope::PREPROCESS, ScopedKeys::CODE_INSIGHTS)
            .await
            .expect("memory of CODE_INSIGHTS not found in PREPROCESS");

        // Collect all code paths associated with this domain
        let mut domain_paths: HashSet<String> = HashSet::new();

        // 1. Add the domain's own code paths
        for path in &domain.code_paths {
            domain_paths.insert(path.clone());
        }

        // 2. Add submodule code paths
        for sub in &domain.sub_modules {
            for path in &sub.code_paths {
                domain_paths.insert(path.clone());
            }
        }

        if domain_paths.is_empty() {
            let msg = context.config.target_language.msg_no_code_path_for_domain();
            println!("{}", msg.replace("{}", &domain.name));
            return Ok(Vec::new());
        }

        let filtered: Vec<CodeInsight> = all_insights
            .into_iter()
            .filter(|insight| {
                let file_path = insight.code_dossier.file_path.to_string_lossy();
                let file_path = file_path.replace('\\', "/");
                domain_paths.iter().any(|path| {
                    let path = path.replace('\\', "/");
                    file_path.contains(&path) || path.contains(&file_path)
                })
            })
            .take(50)
            .collect();

        println!(
            "📁 Filtered {} related code files for domain '{}'",
            filtered.len(),
            domain.name
        );
        Ok(filtered)
    }

    // Execute analysis for a single domain module
    async fn analyze_single_domain(
        &self,
        domain: &DomainModule,
        context: &GeneratorContext,
    ) -> Result<KeyModuleReport> {
        // 1. Filter code insights related to this domain
        let filtered_insights = self
            .filter_code_insights_for_domain(domain, context)
            .await?;

        // 2. Build domain-specific prompt
        let (system_prompt, user_prompt) = self.build_domain_prompt(domain, &filtered_insights);

        // 3. Use agent_executor::extract for analysis
        let params = AgentExecuteParams {
            prompt_sys: system_prompt,
            prompt_user: user_prompt,
            cache_scope: format!(
                "{}/{}/{}",
                crate::generator::research::memory::MemoryScope::STUDIES_RESEARCH,
                self.agent_type(),
                domain.name
            ),
            log_tag: format!("{} domain analysis", domain.name),
            model_preference: crate::generator::step_forward_agent::ModelPreference::Efficient,
        };

        println!("🤖 Analyzing '{}' domain...", domain.name);
        let mut report: KeyModuleReport = extract(context, params).await?;

        // 4. Set domain context information
        report.domain_name = domain.name.clone();
        if report.module_name.is_empty() {
            report.module_name = format!("{} Core Module", domain.name);
        }

        println!("✅ '{}' domain analysis completed", domain.name);
        Ok(report)
    }

    // Build domain-specific prompt
    fn build_domain_prompt(
        &self,
        domain: &DomainModule,
        insights: &[CodeInsight],
    ) -> (String, String) {
        let system_prompt =
            "Based on the information provided by the user, conduct in-depth and rigorous analysis and provide results in the specified format. For each finding, cite the specific file path and function name. Do NOT fabricate capabilities or technologies not evidenced in the code.".to_string();

        let user_prompt = format!(
            "## Domain Analysis Task\nAnalyze the core module technical details of the '{}' domain\n\n### Domain Information\n- Domain Name: {}\n- Domain Type: {}\n- Importance: {:.1}/10\n- Complexity: {:.1}/10\n- Description: {}\n\n### Submodule Overview\n{}\n\n### Related Code Insights\n{}\n",
            domain.name,
            domain.name,
            domain.domain_type,
            domain.importance,
            domain.complexity,
            domain.description,
            self.format_sub_modules(&domain.sub_modules),
            self.format_filtered_insights(insights)
        );

        (system_prompt, user_prompt)
    }

    // Format submodule information
    fn format_sub_modules(&self, sub_modules: &[SubModule]) -> String {
        if sub_modules.is_empty() {
            return "No submodule information available".to_string();
        }

        sub_modules.iter()
            .enumerate()
            .map(|(i, sub)| format!(
                "{}. **{}**\n   - Description: {}\n   - Importance: {:.1}/10\n   - Core Functions: {}\n   - Code Files: {}",
                i + 1,
                sub.name,
                sub.description,
                sub.importance,
                sub.key_functions.join(", "),
                sub.code_paths.join(", ")
            ))
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    // Format filtered code insights
    fn format_filtered_insights(&self, insights: &[CodeInsight]) -> String {
        if insights.is_empty() {
            return "No related code insights available".to_string();
        }

        insights
            .iter()
            .enumerate()
            .map(|(i, insight)| {
                format!(
                    "{}. File `{}`, Purpose: {}\n   Description: {}\n   Source Code\n```code\n{}```\n---\n",
                    i + 1,
                    insight.code_dossier.file_path.to_string_lossy(),
                    insight.code_dossier.code_purpose,
                    insight.detailed_description,
                    insight.code_dossier.source_summary
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}
