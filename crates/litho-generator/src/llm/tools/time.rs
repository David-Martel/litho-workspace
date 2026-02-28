//! Time query tool

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::llm::client::chat_types::ToolDefinition;
use crate::llm::tools::AgentTool;

/// Time tool
#[derive(Debug, Clone)]
pub struct AgentToolTime;

/// Time query parameters
#[derive(Debug, Deserialize)]
pub struct TimeArgs {
    #[serde(rename = "format")]
    pub format: Option<String>,
}

/// Time query result
#[derive(Debug, Serialize)]
pub struct TimeResult {
    pub current_time: String,
    pub timestamp: u64,
    pub utc_time: String,
}

impl AgentToolTime {
    pub fn new() -> Self {
        Self
    }

    async fn get_current_time(&self, args: &TimeArgs) -> Result<TimeResult> {
        // Get current system time
        let now = SystemTime::now();
        let timestamp = now.duration_since(UNIX_EPOCH)?.as_secs();

        // Format time
        let format = args.format.as_deref().unwrap_or("%Y-%m-%d %H:%M:%S");

        // Local time
        let datetime: chrono::DateTime<chrono::Local> = now.into();
        let current_time = datetime.format(format).to_string();

        // UTC time
        let utc_datetime: chrono::DateTime<chrono::Utc> = now.into();
        let utc_time = utc_datetime.format(format).to_string();

        Ok(TimeResult {
            current_time,
            timestamp,
            utc_time,
        })
    }
}

#[async_trait]
impl AgentTool for AgentToolTime {
    fn name(&self) -> &str {
        "time"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "time".to_string(),
            description: "Get current date and time information, including local time, UTC time, and timestamp.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "format": {
                        "type": "string",
                        "description": "Time format string (default is '%Y-%m-%d %H:%M:%S'). Supports chrono formatting syntax."
                    }
                },
                "required": []
            }),
        }
    }

    async fn call_json(&self, arguments: &str) -> Result<String> {
        let args: TimeArgs = serde_json::from_str(arguments)?;
        println!("   🔧 tool called...time@{:?}", args);

        tokio::time::sleep(Duration::from_secs(1)).await;

        let result = self.get_current_time(&args).await?;
        Ok(serde_json::to_string(&result)?)
    }
}
