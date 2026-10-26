pub mod error {
    pub use litho_qmd_storage::{QmdError, QmdResult};
}

pub mod model {
    pub use litho_qmd_storage::{
        CleanupReport, CollectionMutation, CollectionRecord, CollectionUpdateResult,
        ContextMutation, ContextRecord, ContextTarget, DocumentContent, DocumentRequest,
        EmbeddingReport, IndexStatus, IngestReport, ModelPullResult, ModelStatus, MultiGetRequest,
        MultiGetResponse, SearchHit, SearchMode, SearchOptions, SearchResponse,
    };
}

pub mod repo_config {
    pub use litho_qmd_storage::{
        REPO_DOTENV_CANDIDATES, REPO_QMD_CONFIG_CANDIDATES, RepoDatabaseConfig, RepoLlmConfig,
        RepoPathsConfig, RepoQmdConfig, discover_repo_file, get_dotenv_or_env,
        load_repo_dotenv_values, load_repo_json_config, load_repo_qmd_config,
    };
}

pub mod service {
    pub use litho_qmd_storage::QmdService;
}

pub mod traits {
    pub use litho_qmd_storage::{QmdLlmEngine, QmdStore};
}

pub use litho_qmd_storage::{
    CleanupReport, CollectionMutation, CollectionRecord, CollectionUpdateResult, ContextMutation,
    ContextRecord, ContextTarget, DocumentContent, DocumentRequest, EmbeddingReport, IndexStatus,
    IngestReport, ModelPullResult, ModelStatus, MultiGetRequest, MultiGetResponse, QmdError,
    QmdLlmEngine, QmdResult, QmdService, QmdStore, REPO_DOTENV_CANDIDATES,
    REPO_QMD_CONFIG_CANDIDATES, RepoDatabaseConfig, RepoLlmConfig, RepoPathsConfig, RepoQmdConfig,
    SearchHit, SearchMode, SearchOptions, SearchResponse, discover_repo_file, get_dotenv_or_env,
    load_repo_dotenv_values, load_repo_json_config, load_repo_qmd_config,
};
