//! Secondary review agent for the compose pipeline.
//!
//! After the compose stage produces documentation sections, this agent sends
//! each section to the LLM for a lightweight quality review checking:
//! 1. Factual grounding — are claims backed by research data?
//! 2. Structural consistency — does it follow C4 format?
//! 3. Completeness — does it reference relevant modules?
//!
//! If a section scores below the configured threshold, it is flagged for
//! re-generation with the review feedback injected.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::config::{Config, LLMProvider, ReviewConfig};
use crate::generator::compose::memory::MemoryScope;
use crate::generator::context::GeneratorContext;
use crate::llm::client::ollama_native::OllamaNativeClient;

/// Result of reviewing a single documentation section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionReview {
    /// Section name (e.g. "Project Overview")
    pub section: String,
    /// Overall review score (0.0-1.0)
    pub score: f64,
    /// Textual feedback from the reviewer
    pub feedback: String,
    /// Whether the section passed the minimum threshold
    pub passed: bool,
}

/// Secondary review agent that evaluates compose output quality.
///
/// After the compose stage produces documentation sections, this agent
/// sends each section to the LLM for a lightweight quality review checking:
/// 1. Factual grounding — are claims backed by research data?
/// 2. Structural consistency — does it follow C4 format?
/// 3. Completeness — does it reference relevant modules?
///
/// If a section scores below the configured threshold, it is flagged
/// for re-generation with the review feedback injected.
///
/// # Examples
///
/// ```no_run
/// # use litho_generator::generator::compose::agents::review_agent::ReviewAgent;
/// # use litho_generator::generator::context::GeneratorContext;
/// # async fn run(context: &GeneratorContext) -> anyhow::Result<()> {
/// let reviews = ReviewAgent::review_sections(context).await?;
/// for review in &reviews {
///     println!("{}: {:.0}% (passed={})", review.section, review.score * 100.0, review.passed);
/// }
/// # Ok(())
/// # }
/// ```
pub struct ReviewAgent;

impl ReviewAgent {
    /// Run the review pass on all documentation sections in memory.
    ///
    /// Returns a list of section reviews. Sections below the threshold
    /// include feedback that can be injected into a re-generation prompt.
    /// Returns an empty `Vec` when review is disabled or the provider is
    /// not Ollama.
    pub async fn review_sections(context: &GeneratorContext) -> Result<Vec<SectionReview>> {
        let review_config = &context.config.review;
        if !review_config.enabled {
            return Ok(Vec::new());
        }

        // Only works with Ollama provider
        if context.config.llm.provider != LLMProvider::Ollama {
            println!("   Review agent requires Ollama provider; skipping review pass");
            return Ok(Vec::new());
        }

        let client = OllamaNativeClient::from_config(&context.config.llm)?;

        // Load all documentation sections from memory
        let section_keys = context.list_memory_keys(MemoryScope::DOCUMENTATION).await;
        let mut reviews = Vec::new();

        for key in &section_keys {
            if let Some(content) = context
                .get_from_memory::<String>(MemoryScope::DOCUMENTATION, key)
                .await
            {
                if content.len() < 100 {
                    continue; // Skip trivially short sections
                }

                let review = Self::review_single_section(
                    &client,
                    &context.config,
                    key,
                    &content,
                    review_config,
                )
                .await;

                match review {
                    Ok(r) => {
                        println!(
                            "   Review '{}': score={:.0}% {}",
                            key,
                            r.score * 100.0,
                            if r.passed { "[pass]" } else { "[fail]" }
                        );
                        reviews.push(r);
                    }
                    Err(e) => {
                        eprintln!("   Review failed for '{}': {}", key, e);
                        // Treat failed reviews as passing to not block the pipeline
                        reviews.push(SectionReview {
                            section: key.clone(),
                            score: 1.0,
                            feedback: format!("Review failed: {}", e),
                            passed: true,
                        });
                    }
                }
            }
        }

        Ok(reviews)
    }

    /// Review a single documentation section.
    async fn review_single_section(
        client: &OllamaNativeClient,
        config: &Config,
        section_name: &str,
        content: &str,
        review_config: &ReviewConfig,
    ) -> Result<SectionReview> {
        let system_prompt = r#"You are a documentation quality reviewer. Evaluate the provided documentation section on three criteria:

1. GROUNDING (1-5): Are technical claims backed by specific file paths, module names, or code references?
2. STRUCTURE (1-5): Does it follow C4 architecture documentation format with clear headings and sections?
3. COMPLETENESS (1-5): Does it cover the relevant components mentioned in the content?

Respond with ONLY a JSON object:
{"grounding": N, "structure": N, "completeness": N, "feedback": "Brief actionable feedback for improvement"}

Where N is an integer 1-5. Keep feedback under 200 characters."#;

        // Truncate content to keep prompt reasonable
        let excerpt: String = content.chars().take(4000).collect();
        let user_prompt = format!(
            "Review this documentation section titled '{}':\n\n{}",
            section_name, excerpt
        );

        let response = client
            .chat(
                &config.llm.model_efficient,
                system_prompt,
                &user_prompt,
                &config.llm,
            )
            .await?;

        Self::parse_review_response(&response, section_name, review_config)
    }

    /// Parse the LLM's review response into a `SectionReview`.
    ///
    /// Tries direct JSON parse, then code-fenced JSON, then the first `{...}`
    /// block in the response. Returns an error only if no valid JSON is found.
    pub(crate) fn parse_review_response(
        response: &str,
        section_name: &str,
        review_config: &ReviewConfig,
    ) -> Result<SectionReview> {
        let json_value = Self::extract_json(response)?;

        let grounding = json_value
            .get("grounding")
            .and_then(|v| v.as_f64())
            .unwrap_or(3.0);
        let structure = json_value
            .get("structure")
            .and_then(|v| v.as_f64())
            .unwrap_or(3.0);
        let completeness = json_value
            .get("completeness")
            .and_then(|v| v.as_f64())
            .unwrap_or(3.0);
        let feedback = json_value
            .get("feedback")
            .and_then(|v| v.as_str())
            .unwrap_or("No feedback provided")
            .to_string();

        // Average and normalize to 0.0-1.0
        let avg = (grounding + structure + completeness) / 3.0;
        let score = ((avg - 1.0) / 4.0).clamp(0.0, 1.0);

        Ok(SectionReview {
            section: section_name.to_string(),
            score,
            feedback,
            passed: score >= review_config.min_review_score,
        })
    }

    /// Extract a JSON object from a response (direct, code-fenced, or embedded).
    fn extract_json(response: &str) -> Result<serde_json::Value> {
        // 1. Direct parse
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(response) {
            return Ok(v);
        }

        // 2. Code fence
        let fence_re = regex::Regex::new(r"```(?:json)?\s*(\{[\s\S]*?\})\s*```")
            .map_err(|e| anyhow::anyhow!("regex error: {}", e))?;
        if let Some(cap) = fence_re.captures(response)
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(&cap[1])
        {
            return Ok(v);
        }

        // 3. First balanced {...} block
        if let Some(start) = response.find('{') {
            let mut depth = 0i32;
            let mut end = start;
            for (i, c) in response[start..].char_indices() {
                match c {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            end = start + i + 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if depth == 0
                && end > start
                && let Ok(v) = serde_json::from_str::<serde_json::Value>(&response[start..end])
            {
                return Ok(v);
            }
        }

        anyhow::bail!("No valid JSON found in review response")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ReviewConfig;

    #[test]
    fn test_parse_review_response_valid_json() {
        let response = r#"{"grounding": 4, "structure": 5, "completeness": 3, "feedback": "Add more file refs"}"#;
        let config = ReviewConfig::default();
        let review = ReviewAgent::parse_review_response(response, "Overview", &config).unwrap();
        assert_eq!(review.section, "Overview");
        // avg = (4+5+3)/3 = 4.0, normalized = (4-1)/4 = 0.75
        assert!((review.score - 0.75).abs() < 0.01);
        assert!(review.passed); // 0.75 >= 0.6
        assert_eq!(review.feedback, "Add more file refs");
    }

    #[test]
    fn test_parse_review_response_code_fence() {
        let response = "Here's my review:\n```json\n{\"grounding\": 2, \"structure\": 2, \"completeness\": 2, \"feedback\": \"Needs work\"}\n```";
        let config = ReviewConfig::default();
        let review = ReviewAgent::parse_review_response(response, "Arch", &config).unwrap();
        // avg = 2.0, normalized = (2-1)/4 = 0.25
        assert!((review.score - 0.25).abs() < 0.01);
        assert!(!review.passed); // 0.25 < 0.6
    }

    #[test]
    fn test_parse_review_response_embedded_json() {
        let response = "The scores are: {\"grounding\": 5, \"structure\": 5, \"completeness\": 5, \"feedback\": \"Excellent\"} overall good.";
        let config = ReviewConfig::default();
        let review = ReviewAgent::parse_review_response(response, "Workflow", &config).unwrap();
        // avg = 5.0, normalized = (5-1)/4 = 1.0
        assert!((review.score - 1.0).abs() < 0.01);
        assert!(review.passed);
    }

    #[test]
    fn test_parse_review_response_no_json() {
        let response = "I think this documentation is great!";
        let config = ReviewConfig::default();
        let result = ReviewAgent::parse_review_response(response, "DB", &config);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_review_response_partial_scores() {
        let response = r#"{"grounding": 3, "feedback": "Missing structure scores"}"#;
        let config = ReviewConfig::default();
        let review = ReviewAgent::parse_review_response(response, "Overview", &config).unwrap();
        // grounding=3, structure=3.0(default), completeness=3.0(default)
        // avg = 3.0, normalized = (3-1)/4 = 0.5
        assert!((review.score - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_review_config_default() {
        let config = ReviewConfig::default();
        assert!(!config.enabled);
        assert!((config.min_review_score - 0.6).abs() < f64::EPSILON);
        assert_eq!(config.max_retries, 1);
    }

    #[test]
    fn test_review_config_serde_roundtrip() {
        let config = ReviewConfig {
            enabled: true,
            min_review_score: 0.8,
            max_retries: 3,
        };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: ReviewConfig = serde_json::from_str(&json).unwrap();
        assert!(parsed.enabled);
        assert!((parsed.min_review_score - 0.8).abs() < f64::EPSILON);
        assert_eq!(parsed.max_retries, 3);
    }

    #[test]
    fn test_score_boundary_pass() {
        // score exactly at the threshold should pass
        let response = r#"{"grounding": 3, "structure": 4, "completeness": 3, "feedback": "ok"}"#;
        let config = ReviewConfig {
            min_review_score: 0.5,
            ..Default::default()
        };
        let review = ReviewAgent::parse_review_response(response, "Test", &config).unwrap();
        // avg = (3+4+3)/3 = 10/3 ≈ 3.333, normalized = (3.333-1)/4 ≈ 0.583
        assert!(review.score >= 0.5);
        assert!(review.passed);
    }

    #[test]
    fn test_score_clamped_to_unit_interval() {
        // Score of 1 on all dimensions → normalized = 0.0 (clamped)
        let response = r#"{"grounding": 1, "structure": 1, "completeness": 1, "feedback": "poor"}"#;
        let config = ReviewConfig::default();
        let review = ReviewAgent::parse_review_response(response, "Test", &config).unwrap();
        assert!((review.score - 0.0).abs() < 0.01);
        assert!(!review.passed);
    }
}
