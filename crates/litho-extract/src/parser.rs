use anyhow::{Context, Result, bail};
use litho_core::config::{ExtractBackend, LithoConfig};
use litho_core::types::Language;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstGrepInterfaceHint {
    pub name: String,
    pub kind: String,
    pub visibility: String,
    pub signature: String,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstGrepDependencyHint {
    pub target: String,
    pub kind: String,
}

#[derive(Debug, Clone, Default)]
pub struct AstGrepFileHints {
    pub interfaces: Vec<AstGrepInterfaceHint>,
    pub dependencies: Vec<AstGrepDependencyHint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueryKind {
    Interface {
        kind: &'static str,
        visibility: &'static str,
    },
    Dependency {
        kind: &'static str,
    },
}

#[derive(Debug, Clone, Copy)]
struct QuerySpec {
    pattern: &'static str,
    capture_key: &'static str,
    kind: QueryKind,
}

type RunnerFn = dyn Fn(&str, &str, &str, &[PathBuf]) -> Result<String> + Send + Sync;

pub fn collect_ast_grep_hints_for_files(
    files: &[(PathBuf, Language)],
    config: &LithoConfig,
) -> Result<HashMap<PathBuf, AstGrepFileHints>> {
    if matches!(config.extract_backend, ExtractBackend::TreeSitter) {
        return Ok(HashMap::new());
    }
    if files.is_empty() {
        return Ok(HashMap::new());
    }

    let binary = resolve_ast_grep_binary(config);
    if !is_binary_available(&binary) {
        bail!("ast-grep binary '{}' is unavailable", binary);
    }

    collect_ast_grep_hints_with_runner(files, &binary, &run_ast_grep_query)
}

fn collect_ast_grep_hints_with_runner(
    files: &[(PathBuf, Language)],
    binary: &str,
    runner: &RunnerFn,
) -> Result<HashMap<PathBuf, AstGrepFileHints>> {
    let mut hints_by_file: HashMap<PathBuf, AstGrepFileHints> = HashMap::new();
    let mut groups: BTreeMap<&'static str, Vec<PathBuf>> = BTreeMap::new();

    for (path, language) in files {
        if let Some((lang_name, _)) = language_config(language) {
            groups.entry(lang_name).or_default().push(path.clone());
        }
    }

    for (lang_name, paths) in groups {
        let Some((_, queries)) = language_config_by_name(lang_name) else {
            continue;
        };
        for query in queries {
            let output = runner(binary, lang_name, query.pattern, &paths).with_context(|| {
                format!(
                    "ast-grep query failed (lang={}, pattern={})",
                    lang_name, query.pattern
                )
            })?;
            let matches = parse_stream_matches(&output)
                .context("failed to parse ast-grep JSON stream output")?;

            for entry in matches {
                let Some(capture) = entry.capture_value(query.capture_key).map(str::to_string)
                else {
                    continue;
                };
                let file_path = PathBuf::from(&entry.file);
                let file_hints = hints_by_file.entry(file_path).or_default();
                match query.kind {
                    QueryKind::Interface { kind, visibility } => {
                        file_hints.interfaces.push(AstGrepInterfaceHint {
                            name: capture.trim().to_string(),
                            kind: kind.to_string(),
                            visibility: visibility.to_string(),
                            signature: entry.text.trim().to_string(),
                            line: entry.range.start.line.saturating_add(1),
                        });
                    }
                    QueryKind::Dependency { kind } => {
                        let target = normalize_dependency_target(&capture);
                        if !target.is_empty() {
                            file_hints.dependencies.push(AstGrepDependencyHint {
                                target,
                                kind: kind.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    dedup_hints(&mut hints_by_file);
    Ok(hints_by_file)
}

fn resolve_ast_grep_binary(config: &LithoConfig) -> String {
    if let Some(ref bin) = config.ast_grep_binary
        && !bin.trim().is_empty()
    {
        return bin.trim().to_string();
    }
    if let Ok(env_bin) = std::env::var("LITHO_AST_GREP_BIN")
        && !env_bin.trim().is_empty()
    {
        return env_bin.trim().to_string();
    }
    "sg".to_string()
}

fn is_binary_available(binary: &str) -> bool {
    Command::new(binary)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn run_ast_grep_query(
    binary: &str,
    lang: &str,
    pattern: &str,
    paths: &[PathBuf],
) -> Result<String> {
    let mut cmd = Command::new(binary);
    cmd.arg("run")
        .arg("--pattern")
        .arg(pattern)
        .arg("--lang")
        .arg(lang)
        .arg("--json=stream");
    for path in paths {
        cmd.arg(path);
    }

    let output = cmd.output().with_context(|| {
        format!(
            "failed to execute ast-grep (binary='{}', lang='{}')",
            binary, lang
        )
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "ast-grep exited with status {}: {}",
            output.status,
            stderr.trim()
        );
    }
    String::from_utf8(output.stdout).context("ast-grep output was not valid UTF-8")
}

fn normalize_dependency_target(raw: &str) -> String {
    raw.trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .to_string()
}

fn dedup_hints(hints: &mut HashMap<PathBuf, AstGrepFileHints>) {
    for file_hints in hints.values_mut() {
        let mut seen_interfaces = HashSet::new();
        file_hints.interfaces.retain(|interface| {
            seen_interfaces.insert((
                interface.name.clone(),
                interface.kind.clone(),
                interface.line,
            ))
        });

        let mut seen_deps = HashSet::new();
        file_hints
            .dependencies
            .retain(|dep| seen_deps.insert((dep.target.clone(), dep.kind.clone())));
    }
}

fn language_config(language: &Language) -> Option<(&'static str, &'static [QuerySpec])> {
    match language {
        Language::Rust => Some(("rust", RUST_QUERIES)),
        Language::TypeScript => Some(("typescript", TS_QUERIES)),
        Language::Python => Some(("python", PY_QUERIES)),
        Language::CSharp => Some(("csharp", CSHARP_QUERIES)),
        _ => None,
    }
}

fn language_config_by_name(name: &str) -> Option<(&'static str, &'static [QuerySpec])> {
    match name {
        "rust" => Some(("rust", RUST_QUERIES)),
        "typescript" => Some(("typescript", TS_QUERIES)),
        "python" => Some(("python", PY_QUERIES)),
        "csharp" => Some(("csharp", CSHARP_QUERIES)),
        _ => None,
    }
}

const RUST_QUERIES: &[QuerySpec] = &[
    QuerySpec {
        pattern: "pub fn $NAME($$$ARGS) -> $RET {$$$BODY}",
        capture_key: "NAME",
        kind: QueryKind::Interface {
            kind: "function",
            visibility: "pub",
        },
    },
    QuerySpec {
        pattern: "pub fn $NAME($$$ARGS) {$$$BODY}",
        capture_key: "NAME",
        kind: QueryKind::Interface {
            kind: "function",
            visibility: "pub",
        },
    },
    QuerySpec {
        pattern: "pub struct $NAME {$$$BODY}",
        capture_key: "NAME",
        kind: QueryKind::Interface {
            kind: "struct",
            visibility: "pub",
        },
    },
    QuerySpec {
        pattern: "pub trait $NAME {$$$BODY}",
        capture_key: "NAME",
        kind: QueryKind::Interface {
            kind: "trait",
            visibility: "pub",
        },
    },
    QuerySpec {
        pattern: "pub enum $NAME {$$$BODY}",
        capture_key: "NAME",
        kind: QueryKind::Interface {
            kind: "enum",
            visibility: "pub",
        },
    },
    QuerySpec {
        pattern: "use $PATH;",
        capture_key: "PATH",
        kind: QueryKind::Dependency { kind: "use" },
    },
];

const TS_QUERIES: &[QuerySpec] = &[
    QuerySpec {
        pattern: "export function $NAME($$$ARGS) {$$$BODY}",
        capture_key: "NAME",
        kind: QueryKind::Interface {
            kind: "function",
            visibility: "export",
        },
    },
    QuerySpec {
        pattern: "export class $NAME {$$$BODY}",
        capture_key: "NAME",
        kind: QueryKind::Interface {
            kind: "class",
            visibility: "export",
        },
    },
    QuerySpec {
        pattern: "export interface $NAME {$$$BODY}",
        capture_key: "NAME",
        kind: QueryKind::Interface {
            kind: "interface",
            visibility: "export",
        },
    },
    QuerySpec {
        pattern: "import $NAME from $SOURCE;",
        capture_key: "SOURCE",
        kind: QueryKind::Dependency { kind: "import" },
    },
    QuerySpec {
        pattern: "import {$$$NAMES} from $SOURCE;",
        capture_key: "SOURCE",
        kind: QueryKind::Dependency { kind: "import" },
    },
    QuerySpec {
        pattern: "import * as $NAME from $SOURCE;",
        capture_key: "SOURCE",
        kind: QueryKind::Dependency { kind: "import" },
    },
];

const PY_QUERIES: &[QuerySpec] = &[
    QuerySpec {
        pattern: "def $NAME($$$ARGS): $$$BODY",
        capture_key: "NAME",
        kind: QueryKind::Interface {
            kind: "function",
            visibility: "public",
        },
    },
    QuerySpec {
        pattern: "class $NAME($$$ARGS): $$$BODY",
        capture_key: "NAME",
        kind: QueryKind::Interface {
            kind: "class",
            visibility: "public",
        },
    },
    QuerySpec {
        pattern: "import $MODULE",
        capture_key: "MODULE",
        kind: QueryKind::Dependency { kind: "import" },
    },
    QuerySpec {
        pattern: "from $MODULE import $$$NAMES",
        capture_key: "MODULE",
        kind: QueryKind::Dependency { kind: "import" },
    },
];

const CSHARP_QUERIES: &[QuerySpec] = &[
    QuerySpec {
        pattern: "public class $NAME {$$$BODY}",
        capture_key: "NAME",
        kind: QueryKind::Interface {
            kind: "class",
            visibility: "public",
        },
    },
    QuerySpec {
        pattern: "public interface $NAME {$$$BODY}",
        capture_key: "NAME",
        kind: QueryKind::Interface {
            kind: "interface",
            visibility: "public",
        },
    },
    QuerySpec {
        pattern: "public struct $NAME {$$$BODY}",
        capture_key: "NAME",
        kind: QueryKind::Interface {
            kind: "struct",
            visibility: "public",
        },
    },
    QuerySpec {
        pattern: "public enum $NAME {$$$BODY}",
        capture_key: "NAME",
        kind: QueryKind::Interface {
            kind: "enum",
            visibility: "public",
        },
    },
    QuerySpec {
        pattern: "using $NAMESPACE;",
        capture_key: "NAMESPACE",
        kind: QueryKind::Dependency { kind: "using" },
    },
];

#[derive(Debug, Deserialize)]
struct AstGrepMatchEntry {
    text: String,
    file: String,
    range: AstGrepRange,
    #[serde(default, rename = "metaVariables")]
    meta_variables: AstGrepMetaVariables,
}

#[derive(Debug, Deserialize)]
struct AstGrepRange {
    start: AstGrepPosition,
}

#[derive(Debug, Deserialize)]
struct AstGrepPosition {
    line: usize,
}

#[derive(Debug, Deserialize, Default)]
struct AstGrepMetaVariables {
    #[serde(default)]
    single: HashMap<String, AstGrepCapture>,
}

#[derive(Debug, Deserialize)]
struct AstGrepCapture {
    text: String,
}

impl AstGrepMatchEntry {
    fn capture_value(&self, key: &str) -> Option<&str> {
        self.meta_variables.single.get(key).map(|v| v.text.as_str())
    }
}

fn parse_stream_matches(output: &str) -> Result<Vec<AstGrepMatchEntry>> {
    let mut results = Vec::new();
    for line in output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let entry: AstGrepMatchEntry =
            serde_json::from_str(line).with_context(|| format!("invalid JSON line: {}", line))?;
        results.push(entry);
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use litho_core::types::Language;
    use std::sync::{Arc, Mutex};

    #[test]
    fn parse_stream_matches_parses_json_lines() {
        let out = r#"{"text":"use std::path::Path;","file":"src/main.rs","range":{"start":{"line":3}},"metaVariables":{"single":{"PATH":{"text":"std::path::Path"}}}}
{"text":"use crate::config::Cfg;","file":"src/main.rs","range":{"start":{"line":7}},"metaVariables":{"single":{"PATH":{"text":"crate::config::Cfg"}}}}"#;
        let entries = parse_stream_matches(out).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].capture_value("PATH"), Some("std::path::Path"));
        assert_eq!(entries[1].range.start.line, 7);
    }

    #[test]
    fn collect_hints_with_runner_aggregates_interfaces_and_deps() {
        let calls: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let calls_clone = calls.clone();
        let runner = move |_bin: &str,
                           lang: &str,
                           pattern: &str,
                           _paths: &[PathBuf]|
              -> Result<String> {
            calls_clone
                .lock()
                .unwrap()
                .push((lang.to_string(), pattern.to_string()));

            if pattern == "pub struct $NAME {$$$BODY}" {
                return Ok(r#"{"text":"pub struct Settings { x: i32 }","file":"repo/src/lib.rs","range":{"start":{"line":9}},"metaVariables":{"single":{"NAME":{"text":"Settings"}}}}"#.to_string());
            }
            if pattern == "use $PATH;" {
                return Ok(r#"{"text":"use std::path::Path;","file":"repo/src/lib.rs","range":{"start":{"line":1}},"metaVariables":{"single":{"PATH":{"text":"std::path::Path"}}}}"#.to_string());
            }
            Ok(String::new())
        };

        let files = vec![(PathBuf::from("repo/src/lib.rs"), Language::Rust)];
        let hints = collect_ast_grep_hints_with_runner(&files, "sg", &runner).unwrap();
        let file_hints = hints.get(&PathBuf::from("repo/src/lib.rs")).unwrap();
        assert!(file_hints.interfaces.iter().any(|i| i.name == "Settings"));
        assert!(
            file_hints
                .dependencies
                .iter()
                .any(|d| d.target == "std::path::Path")
        );
        assert!(!calls.lock().unwrap().is_empty());
    }

    #[test]
    fn collect_hints_with_runner_bubbles_parse_errors() {
        let runner =
            |_bin: &str, _lang: &str, _pattern: &str, _paths: &[PathBuf]| -> Result<String> {
                Ok("{not-json}".to_string())
            };
        let files = vec![(PathBuf::from("repo/src/lib.rs"), Language::Rust)];
        let err = collect_ast_grep_hints_with_runner(&files, "sg", &runner).unwrap_err();
        assert!(format!("{err:#}").contains("invalid JSON line"));
    }
}
