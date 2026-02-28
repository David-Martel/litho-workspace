use litho_qmd_core::{ModelPullResult, ModelStatus, QmdError, QmdLlmEngine, QmdResult, SearchHit};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Default)]
pub struct NoopLlmEngine;

impl QmdLlmEngine for NoopLlmEngine {
    fn expand_query(&self, query: &str) -> QmdResult<Vec<String>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(vec![]);
        }

        let mut variants = vec![query.to_string()];
        let lower = query.to_lowercase();
        if lower != query {
            variants.push(lower.clone());
        }

        let mut normalized = lower
            .replace("config", "configuration")
            .replace("build", "compile")
            .replace("bug", "issue")
            .replace("perf", "performance");
        normalized = normalized
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string();
        if !normalized.is_empty() && !variants.contains(&normalized) {
            variants.push(normalized);
        }

        let terms = tokenize(query);
        if terms.len() > 2 {
            let compact = terms.join(" ");
            if !variants.contains(&compact) {
                variants.push(compact);
            }
        }

        Ok(variants)
    }

    fn rerank(&self, query: &str, mut candidates: Vec<SearchHit>) -> QmdResult<Vec<SearchHit>> {
        let query_terms = tokenize(query);
        for hit in &mut candidates {
            let hay = format!("{} {}", hit.title, hit.snippet).to_lowercase();
            let overlap = query_terms
                .iter()
                .filter(|term| hay.contains(term.as_str()))
                .count();
            let overlap_score = if query_terms.is_empty() {
                0.0
            } else {
                overlap as f32 / query_terms.len() as f32
            };
            hit.score = 0.78 * hit.score + 0.22 * overlap_score;
        }
        candidates.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.file.cmp(&b.file)));
        Ok(candidates)
    }

    fn local_models(&self) -> QmdResult<Vec<ModelStatus>> {
        let roots = model_roots();
        let preferred_files = [
            (
                "qmd-embed-default",
                "hf_ggml-org_embeddinggemma-300M-Q8_0.gguf",
            ),
            (
                "qmd-rerank-default",
                "hf_ggml-org_qwen3-reranker-0.6b-q8_0.gguf",
            ),
            (
                "qmd-query-default",
                "hf_tobil_qmd-query-expansion-1.7B-q4_k_m.gguf",
            ),
            ("qmd-query-qwen-3b", "Qwen2.5-Coder-3B-Instruct-Q4_K_M.gguf"),
        ];

        let mut out = Vec::new();
        for (name, file_name) in preferred_files {
            let found = roots
                .iter()
                .map(|root| root.join(file_name))
                .find(|candidate| candidate.exists());
            out.push(ModelStatus {
                name: name.to_string(),
                available: found.is_some(),
                location: found.map(|p| p.to_string_lossy().to_string()),
            });
        }

        let discovered = discover_models(&roots, 64);
        for (idx, path) in discovered.into_iter().enumerate() {
            out.push(ModelStatus {
                name: format!("local:{}", idx + 1),
                available: true,
                location: Some(path.to_string_lossy().to_string()),
            });
        }

        Ok(out)
    }

    fn pull_models(&self, refresh: bool) -> QmdResult<Vec<ModelPullResult>> {
        let mut results = Vec::new();
        let ollama_models = required_ollama_models();
        let installed = ollama_list_models().unwrap_or_default();

        for model in ollama_models {
            if !refresh && installed.contains(&model) {
                results.push(ModelPullResult {
                    name: model,
                    success: true,
                    note: "already present in ollama cache".to_string(),
                });
                continue;
            }

            let status = Command::new("ollama").arg("pull").arg(&model).status();
            match status {
                Ok(code) if code.success() => results.push(ModelPullResult {
                    name: model,
                    success: true,
                    note: if refresh {
                        "refreshed via ollama pull".to_string()
                    } else {
                        "pulled via ollama".to_string()
                    },
                }),
                Ok(code) => results.push(ModelPullResult {
                    name: model,
                    success: false,
                    note: format!("ollama pull exited with {code}"),
                }),
                Err(err) => results.push(ModelPullResult {
                    name: model,
                    success: false,
                    note: format!("ollama unavailable: {err}"),
                }),
            }
        }
        Ok(results)
    }
}

#[derive(Debug, Clone)]
pub struct AdaptiveLlmEngine {
    mode: ProviderMode,
    fallback: NoopLlmEngine,
    ollama: Option<OllamaClient>,
    ollama_complexity_threshold: usize,
    force_ollama: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderMode {
    Noop,
    Ollama,
    Hybrid,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RepoQmdConfig {
    #[serde(default)]
    llm: RepoLlmConfig,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RepoLlmConfig {
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    ollama_query_model: Option<String>,
    #[serde(default)]
    ollama_rerank_model: Option<String>,
    #[serde(default)]
    ollama_required_models: Vec<String>,
    #[serde(default)]
    model_dirs: Vec<String>,
    #[serde(default)]
    ollama_complexity_threshold: Option<usize>,
    #[serde(default)]
    force_ollama: Option<bool>,
}

impl AdaptiveLlmEngine {
    pub fn from_env() -> Self {
        let cfg = repo_qmd_config().llm;
        let mode_raw = cfg
            .provider
            .clone()
            .or_else(|| get_dotenv_or_env("QMD_LLM_PROVIDER"))
            .unwrap_or_else(|| "hybrid".to_string());
        let mode = match mode_raw.to_lowercase().as_str() {
            "noop" => ProviderMode::Noop,
            "ollama" => ProviderMode::Ollama,
            _ => ProviderMode::Hybrid,
        };

        let ollama = if mode != ProviderMode::Noop {
            Some(OllamaClient::from_env())
        } else {
            None
        };

        let threshold = cfg
            .ollama_complexity_threshold
            .or_else(|| {
                get_dotenv_or_env("QMD_LLM_OLLAMA_COMPLEXITY_THRESHOLD")
                    .and_then(|v| v.parse::<usize>().ok())
            })
            .unwrap_or(6)
            .max(3);
        let force_ollama = cfg
            .force_ollama
            .or_else(|| {
                get_dotenv_or_env("QMD_LLM_FORCE_OLLAMA")
                    .as_deref()
                    .map(parse_boolish)
            })
            .unwrap_or(false);

        Self {
            mode,
            fallback: NoopLlmEngine,
            ollama,
            ollama_complexity_threshold: threshold,
            force_ollama,
        }
    }

    fn ollama_enabled(&self) -> bool {
        self.mode == ProviderMode::Ollama || self.mode == ProviderMode::Hybrid
    }

    fn should_use_ollama(&self, query: &str, candidate_count: usize) -> bool {
        if self.force_ollama {
            return true;
        }

        let terms = tokenize(query);
        if terms.len() >= self.ollama_complexity_threshold {
            return true;
        }

        let has_question_intent = ["how", "why", "difference", "tradeoff", "optimize", "design"]
            .iter()
            .any(|k| query.to_lowercase().contains(k));

        has_question_intent || candidate_count > 12
    }
}

impl Default for AdaptiveLlmEngine {
    fn default() -> Self {
        Self::from_env()
    }
}

impl QmdLlmEngine for AdaptiveLlmEngine {
    fn expand_query(&self, query: &str) -> QmdResult<Vec<String>> {
        let mut base = self.fallback.expand_query(query)?;

        if !self.ollama_enabled() || !self.should_use_ollama(query, 0) {
            return Ok(base);
        }

        if let Some(client) = &self.ollama
            && let Ok(extra) = client.expand_query(query)
            && !extra.is_empty()
        {
            base = merge_variants(base, extra, 6);
        }

        Ok(base)
    }

    fn rerank(&self, query: &str, candidates: Vec<SearchHit>) -> QmdResult<Vec<SearchHit>> {
        let baseline = self.fallback.rerank(query, candidates.clone())?;

        if !self.ollama_enabled() || !self.should_use_ollama(query, candidates.len()) {
            return Ok(baseline);
        }

        if let Some(client) = &self.ollama
            && let Ok(result) = client.rerank(query, baseline.clone())
            && !result.is_empty()
        {
            return Ok(result);
        }

        Ok(baseline)
    }

    fn local_models(&self) -> QmdResult<Vec<ModelStatus>> {
        let mut out = self.fallback.local_models()?;

        if self.ollama_enabled()
            && let Ok(models) = ollama_list_models()
        {
            for model in models {
                out.push(ModelStatus {
                    name: format!("ollama:{model}"),
                    available: true,
                    location: Some("ollama".to_string()),
                });
            }
        }

        dedup_model_statuses(&mut out);
        Ok(out)
    }

    fn pull_models(&self, refresh: bool) -> QmdResult<Vec<ModelPullResult>> {
        self.fallback.pull_models(refresh)
    }
}

#[derive(Debug, Clone)]
struct OllamaClient {
    query_model: String,
    rerank_model: String,
}

impl OllamaClient {
    fn from_env() -> Self {
        let cfg = repo_qmd_config().llm;
        let query_model = cfg
            .ollama_query_model
            .clone()
            .or_else(|| get_dotenv_or_env("QMD_OLLAMA_QUERY_MODEL"))
            .unwrap_or_else(|| {
                best_ollama_model_candidate().unwrap_or_else(|| "qwen2.5-coder:3b".into())
            });
        let rerank_model = cfg
            .ollama_rerank_model
            .clone()
            .or_else(|| get_dotenv_or_env("QMD_OLLAMA_RERANK_MODEL"))
            .unwrap_or_else(|| query_model.clone());
        Self {
            query_model,
            rerank_model,
        }
    }

    fn expand_query(&self, query: &str) -> QmdResult<Vec<String>> {
        let prompt = format!(
            "Rewrite the query into 2-4 short search variants. Return JSON array of strings only.\nQuery: {query}"
        );
        let text = self.generate(&self.query_model, &prompt)?;
        let variants = parse_json_string_array(&text).unwrap_or_else(|| vec![query.to_string()]);
        Ok(variants
            .into_iter()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .take(4)
            .collect())
    }

    fn rerank(&self, query: &str, mut candidates: Vec<SearchHit>) -> QmdResult<Vec<SearchHit>> {
        if candidates.is_empty() {
            return Ok(candidates);
        }
        let limited = candidates.iter().take(20).collect::<Vec<_>>();
        let mut payload = String::new();
        for (idx, hit) in limited.iter().enumerate() {
            let snippet = hit.snippet.chars().take(220).collect::<String>();
            payload.push_str(&format!(
                "{idx}|{}|{}\n",
                hit.file,
                snippet.replace('\n', " ")
            ));
        }
        let prompt = format!(
            "Given query and candidates, output JSON array of objects {{\"idx\": number, \"score\": number}} in [0,1].\nQuery: {query}\nCandidates:\n{payload}"
        );
        let response = self.generate(&self.rerank_model, &prompt)?;
        let parsed = parse_json_rerank(&response);

        if parsed.is_empty() {
            return Err(QmdError::Internal(
                "ollama rerank did not return parseable scores".to_string(),
            ));
        }

        let mut score_map = BTreeMap::<usize, f32>::new();
        for (idx, score) in parsed {
            score_map.insert(idx, score.clamp(0.0, 1.0));
        }

        for (idx, hit) in candidates.iter_mut().enumerate() {
            if let Some(score) = score_map.get(&idx) {
                hit.score = 0.62 * hit.score + 0.38 * *score;
            }
        }
        candidates.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.file.cmp(&b.file)));
        Ok(candidates)
    }

    fn generate(&self, model: &str, prompt: &str) -> QmdResult<String> {
        let output = Command::new("ollama")
            .arg("run")
            .arg(model)
            .arg(prompt)
            .output()
            .map_err(|e| QmdError::Internal(format!("failed to start ollama: {e}")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(QmdError::Internal(format!(
                "ollama run failed ({:?}): {}",
                output.status.code(),
                stderr.trim()
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

fn merge_variants(base: Vec<String>, extra: Vec<String>, max_items: usize) -> Vec<String> {
    let mut seen = BTreeSet::<String>::new();
    let mut out = Vec::new();
    for item in base.into_iter().chain(extra.into_iter()) {
        let normalized = item.trim().to_string();
        if normalized.is_empty() {
            continue;
        }
        if seen.insert(normalized.clone()) {
            out.push(normalized);
        }
        if out.len() >= max_items {
            break;
        }
    }
    out
}

fn dedup_model_statuses(items: &mut Vec<ModelStatus>) {
    let mut seen = BTreeSet::<(String, Option<String>)>::new();
    items.retain(|m| seen.insert((m.name.clone(), m.location.clone())));
}

fn tokenize(input: &str) -> Vec<String> {
    input
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|s| s.len() >= 2)
        .map(str::to_string)
        .collect()
}

fn required_ollama_models() -> Vec<String> {
    let cfg = repo_qmd_config().llm;
    if !cfg.ollama_required_models.is_empty() {
        return cfg.ollama_required_models;
    }

    if let Some(raw) = get_dotenv_or_env("QMD_OLLAMA_REQUIRED_MODELS") {
        let parsed = raw
            .split(',')
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .collect::<Vec<_>>();
        if !parsed.is_empty() {
            return parsed;
        }
    }

    vec![
        "nomic-embed-text:latest".to_string(),
        "qwen2.5-coder:3b".to_string(),
    ]
}

fn ollama_list_models() -> QmdResult<Vec<String>> {
    let output = Command::new("ollama")
        .arg("list")
        .output()
        .map_err(|e| QmdError::Internal(format!("failed to run ollama list: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(QmdError::Internal(format!(
            "ollama list failed ({:?}): {}",
            output.status.code(),
            stderr.trim()
        )));
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut models = Vec::new();
    for line in text.lines().skip(1) {
        let name = line.split_whitespace().next().unwrap_or_default();
        if !name.is_empty() {
            models.push(name.to_string());
        }
    }
    Ok(models)
}

fn best_ollama_model_candidate() -> Option<String> {
    let models = ollama_list_models().ok()?;
    let preferred = [
        "qwen2.5-coder:3b",
        "qwen2.5-coder:7b",
        "gemma3:4b",
        "mistral:latest",
    ];

    for item in preferred {
        if models.iter().any(|m| m.eq_ignore_ascii_case(item)) {
            return Some(item.to_string());
        }
    }

    models.into_iter().next()
}

fn model_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let cfg = repo_qmd_config().llm;

    if let Ok(explicit) = env::var("QMD_MODEL_CACHE_DIR") {
        roots.push(PathBuf::from(explicit));
    }

    if !cfg.model_dirs.is_empty() {
        roots.extend(cfg.model_dirs.into_iter().map(PathBuf::from));
    } else if let Ok(custom) = env::var("QMD_MODEL_DIRS") {
        for part in custom
            .split(';')
            .chain(custom.split(':'))
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            roots.push(PathBuf::from(part));
        }
    }

    if let Ok(home) = env::var("HOME").or_else(|_| env::var("USERPROFILE")) {
        let home = PathBuf::from(home);
        roots.push(home.join(".cache").join("qmd").join("models"));
        roots.push(home.join(".ollama").join("models"));
    }

    roots.push(PathBuf::from("C:/codedev/llm/.models"));

    let mut unique = BTreeSet::<String>::new();
    roots
        .into_iter()
        .filter(|p| p.exists())
        .filter(|p| unique.insert(p.to_string_lossy().to_string()))
        .collect()
}

fn discover_models(roots: &[PathBuf], cap: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in roots {
        visit_model_files(root, &mut out, cap);
        if out.len() >= cap {
            break;
        }
    }
    out
}

fn visit_model_files(root: &Path, out: &mut Vec<PathBuf>, cap: usize) {
    if out.len() >= cap {
        return;
    }

    let entries = match std::fs::read_dir(root) {
        Ok(v) => v,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        if out.len() >= cap {
            break;
        }

        let path = entry.path();
        if path.is_dir() {
            if path
                .file_name()
                .and_then(|v| v.to_str())
                .map(|v| v.eq_ignore_ascii_case("blobs"))
                .unwrap_or(false)
            {
                continue;
            }
            visit_model_files(&path, out, cap);
            continue;
        }

        let ext = path
            .extension()
            .and_then(|v| v.to_str())
            .map(|v| v.to_ascii_lowercase())
            .unwrap_or_default();
        if matches!(ext.as_str(), "gguf" | "safetensors" | "onnx") {
            out.push(path);
        }
    }
}

fn parse_json_string_array(raw: &str) -> Option<Vec<String>> {
    let value = extract_json_value(raw)?;
    let arr = value.as_array()?;
    Some(
        arr.iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
    )
}

fn parse_json_rerank(raw: &str) -> Vec<(usize, f32)> {
    let value = match extract_json_value(raw) {
        Some(v) => v,
        None => return Vec::new(),
    };
    let arr = match value.as_array() {
        Some(v) => v,
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    for item in arr {
        let idx = item.get("idx").and_then(Value::as_u64);
        let score = item.get("score").and_then(Value::as_f64);
        if let (Some(i), Some(s)) = (idx, score) {
            out.push((i as usize, s as f32));
        }
    }
    out
}

fn extract_json_value(raw: &str) -> Option<Value> {
    if let Ok(v) = serde_json::from_str::<Value>(raw) {
        return Some(v);
    }
    let start = raw.find('[').or_else(|| raw.find('{'))?;
    let end = raw.rfind(']').or_else(|| raw.rfind('}'))?;
    if end <= start {
        return None;
    }
    serde_json::from_str::<Value>(&raw[start..=end]).ok()
}

fn repo_qmd_config() -> RepoQmdConfig {
    let path = discover_repo_file("qmd.config.json")
        .or_else(|| discover_repo_file("config/qmd.config.json"));
    let Some(path) = path else {
        return RepoQmdConfig::default();
    };

    let raw = match std::fs::read_to_string(path) {
        Ok(v) => v,
        Err(_) => return RepoQmdConfig::default(),
    };
    serde_json::from_str::<RepoQmdConfig>(&raw).unwrap_or_default()
}

fn get_dotenv_or_env(key: &str) -> Option<String> {
    repo_dotenv_values()
        .get(key)
        .cloned()
        .or_else(|| env::var(key).ok())
}

fn repo_dotenv_values() -> BTreeMap<String, String> {
    let env_path = discover_repo_file(".env").or_else(|| discover_repo_file("config/.env"));
    let Some(path) = env_path else {
        return BTreeMap::new();
    };

    let raw = match std::fs::read_to_string(path) {
        Ok(v) => v,
        Err(_) => return BTreeMap::new(),
    };
    let mut map = BTreeMap::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((k, v)) = trimmed.split_once('=') else {
            continue;
        };
        let key = k.trim().to_string();
        let mut value = v.trim().to_string();
        if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
            value = value[1..value.len() - 1].to_string();
        }
        map.insert(key, value);
    }
    map
}

fn discover_repo_file(relative: &str) -> Option<PathBuf> {
    let mut cursor = std::env::current_dir().ok()?;
    loop {
        let candidate = cursor.join(relative);
        if candidate.exists() {
            return Some(candidate);
        }
        if !cursor.pop() {
            break;
        }
    }

    let known = PathBuf::from("C:/codedev/litho-workspace").join(relative);
    if known.exists() {
        return Some(known);
    }
    None
}

fn parse_boolish(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}
