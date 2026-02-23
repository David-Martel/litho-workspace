use anyhow::Result;
use std::fs;

use super::Outlet;
use super::summary_generator::{SummaryContentGenerator, SummaryDataCollector, SummaryMode};
use crate::generator::context::GeneratorContext;

/// Summary outlet - responsible for generating and saving summary reports
pub struct SummaryOutlet {
    /// Relative path to the full version summary file
    full_file_path: String,
    /// Relative path to the brief version summary file
    brief_file_path: String,
    /// Whether to generate both versions
    generate_both: bool,
}

impl SummaryOutlet {
    pub fn new() -> Self {
        Self {
            full_file_path: "__Litho_Summary_Detail__.md".to_string(),
            brief_file_path: "__Litho_Summary_Brief__.md".to_string(),
            generate_both: true,
        }
    }
}

impl Outlet for SummaryOutlet {
    async fn save(&self, context: &GeneratorContext) -> Result<()> {
        // Create output directory
        let output_dir = &context.config.output_path;
        if !output_dir.exists() {
            fs::create_dir_all(output_dir)?;
        }

        println!("\n🖊️ Generating project summary report...");

        // Collect data (only needs to be collected once)
        let summary_data = SummaryDataCollector::collect_data(context).await?;

        // Generate and save full version
        let full_content =
            SummaryContentGenerator::generate_content(&summary_data, SummaryMode::Full);
        let full_path = output_dir.join(&self.full_file_path);
        fs::write(&full_path, full_content)?;
        println!("💾 Saved full version summary report: {}", full_path.display());

        // If brief version needs to be generated
        if self.generate_both {
            let brief_content =
                SummaryContentGenerator::generate_content(&summary_data, SummaryMode::Brief);
            let brief_path = output_dir.join(&self.brief_file_path);
            fs::write(&brief_path, brief_content)?;
            println!("💾 Saved brief version summary report: {}", brief_path.display());
        }

        Ok(())
    }
}

impl Default for SummaryOutlet {
    fn default() -> Self {
        Self::new()
    }
}
