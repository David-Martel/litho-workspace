use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OriginalDocument {
    /// README file content in the project, not necessarily accurate and for reference only
    pub readme: Option<String>,
}