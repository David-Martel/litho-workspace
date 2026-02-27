use anyhow::{Context as _, bail};
use clap::{Args, Parser, Subcommand};
use litho_qmd_core::{
    CollectionMutation, CollectionRecord, ContextMutation, ContextRecord, ContextTarget,
    DocumentRequest, MultiGetRequest, QmdService, SearchOptions,
};
use litho_qmd_llm::AdaptiveLlmEngine;
use litho_qmd_storage::PostgresQmdStore;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

type AppService = QmdService<PostgresQmdStore, AdaptiveLlmEngine>;

#[derive(Debug, Parser)]
#[command(name = "qmd", version, about = "Pure Rust QMD CLI")]
struct Cli {
    #[arg(
        long,
        value_name = "INDEX",
        help = "QMD index name, defaults to 'index'"
    )]
    index: Option<String>,

    #[command(subcommand)]
    command: CommandSet,
}

#[derive(Debug, Subcommand)]
enum CommandSet {
    Context(ContextArgs),
    Get(GetArgs),
    MultiGet(MultiGetArgs),
    Ls(ListArgs),
    Collection(CollectionArgs),
    Status(StatusArgs),
    Update,
    Ingest {
        #[arg(long)]
        force: bool,
    },
    Embed {
        #[arg(long)]
        force: bool,
    },
    Pull {
        #[arg(long)]
        refresh: bool,
    },
    Search(SearchArgs),
    Vsearch(SearchArgs),
    Query(SearchArgs),
    Mcp {
        #[arg(last = true)]
        passthrough: Vec<String>,
    },
    Cleanup {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Args)]
struct SearchArgs {
    query: String,
    #[arg(long, default_value_t = 10)]
    limit: usize,
    #[arg(long = "min-score", default_value_t = 0.0)]
    min_score: f32,
    #[arg(short = 'c', long)]
    collection: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct GetArgs {
    file: String,
    #[arg(long = "from")]
    from_line: Option<usize>,
    #[arg(short = 'l', long = "lines")]
    max_lines: Option<usize>,
    #[arg(long = "line-numbers")]
    line_numbers: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct MultiGetArgs {
    pattern: String,
    #[arg(short = 'l', long = "lines")]
    max_lines: Option<usize>,
    #[arg(long = "max-bytes", default_value_t = 10 * 1024)]
    max_bytes: usize,
    #[arg(long = "line-numbers")]
    line_numbers: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ListArgs {
    prefix: Option<String>,
}

#[derive(Debug, Args)]
struct StatusArgs {
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ContextArgs {
    #[command(subcommand)]
    subcommand: ContextSubcommand,
}

#[derive(Debug, Subcommand)]
enum ContextSubcommand {
    Add {
        #[arg(
            value_name = "TARGET|TEXT",
            required = true,
            num_args = 1..=2,
            trailing_var_arg = true,
            help = "Either '<text>' or '<target> <text>'; target defaults to current directory"
        )]
        args: Vec<String>,
    },
    List {
        #[arg(long)]
        json: bool,
    },
    Check,
    Rm {
        target: String,
    },
}

#[derive(Debug, Args)]
struct CollectionArgs {
    #[command(subcommand)]
    subcommand: CollectionSubcommand,
}

#[derive(Debug, Subcommand)]
enum CollectionSubcommand {
    List {
        #[arg(long)]
        json: bool,
    },
    Add {
        path: PathBuf,
        #[arg(long, default_value = "**/*.md")]
        mask: String,
        #[arg(long)]
        name: Option<String>,
    },
    Remove {
        name: String,
    },
    Rename {
        old_name: String,
        new_name: String,
    },
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("warn")
        .without_time()
        .init();

    let cli = Cli::parse();
    run(cli)
}

fn run(cli: Cli) -> anyhow::Result<()> {
    let index = cli.index.as_deref();

    if let CommandSet::Mcp { passthrough } = cli.command {
        let status = Command::new("litho-qmd-mcp")
            .args(passthrough)
            .status()
            .context("failed to start litho-qmd-mcp; ensure crate is built and on PATH")?;
        if !status.success() {
            bail!("litho-qmd-mcp exited with status {status}");
        }
        return Ok(());
    }

    let store = PostgresQmdStore::open_default(index)
        .context("failed to open qmd postgres/config store")?;
    let service = QmdService::new(store, AdaptiveLlmEngine::from_env());

    match cli.command {
        CommandSet::Context(args) => handle_context(args, &service),
        CommandSet::Get(args) => handle_get(args, &service),
        CommandSet::MultiGet(args) => handle_multi_get(args, &service),
        CommandSet::Ls(args) => handle_ls(args, &service),
        CommandSet::Collection(args) => handle_collection(args, &service),
        CommandSet::Status(args) => handle_status(args, &service),
        CommandSet::Update => handle_update(&service),
        CommandSet::Ingest { force } => handle_ingest(force, &service),
        CommandSet::Embed { force } => handle_embed(force, &service),
        CommandSet::Pull { refresh } => handle_pull(refresh, &service),
        CommandSet::Search(args) => handle_search(args, &service),
        CommandSet::Vsearch(args) => handle_vsearch(args, &service),
        CommandSet::Query(args) => handle_query(args, &service),
        CommandSet::Cleanup { json } => handle_cleanup(json, &service),
        CommandSet::Mcp { .. } => Ok(()),
    }
}

fn handle_search(args: SearchArgs, service: &AppService) -> anyhow::Result<()> {
    let response = service
        .search(
            &args.query,
            SearchOptions {
                limit: args.limit,
                min_score: args.min_score,
                collection: args.collection,
            },
        )
        .map_err(anyhow::Error::msg)?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&response)?);
    } else {
        println!(
            "Found {} result(s) for \"{}\":",
            response.results.len(),
            response.query
        );
        for hit in response.results {
            println!(
                "{} {:.2} {} - {}",
                hit.docid, hit.score, hit.file, hit.title
            );
        }
    }
    Ok(())
}

fn handle_vsearch(args: SearchArgs, service: &AppService) -> anyhow::Result<()> {
    let response = service
        .vsearch(
            &args.query,
            SearchOptions {
                limit: args.limit,
                min_score: args.min_score,
                collection: args.collection,
            },
        )
        .map_err(anyhow::Error::msg)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&response)?);
    } else {
        for hit in response.results {
            println!(
                "{} {:.2} {} - {}",
                hit.docid, hit.score, hit.file, hit.title
            );
        }
    }
    Ok(())
}

fn handle_query(args: SearchArgs, service: &AppService) -> anyhow::Result<()> {
    let response = service
        .query(
            &args.query,
            SearchOptions {
                limit: args.limit,
                min_score: args.min_score,
                collection: args.collection,
            },
        )
        .map_err(anyhow::Error::msg)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&response)?);
    } else {
        for hit in response.results {
            println!(
                "{} {:.2} {} - {}",
                hit.docid, hit.score, hit.file, hit.title
            );
        }
    }
    Ok(())
}

fn handle_get(args: GetArgs, service: &AppService) -> anyhow::Result<()> {
    let doc = service
        .get(&DocumentRequest {
            file: args.file,
            from_line: args.from_line,
            max_lines: args.max_lines,
            line_numbers: args.line_numbers,
        })
        .map_err(anyhow::Error::msg)?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&doc)?);
    } else {
        if let Some(context) = doc.context.as_deref() {
            println!("<!-- Context: {context} -->\n");
        }
        println!("{}", doc.text);
    }
    Ok(())
}

fn handle_multi_get(args: MultiGetArgs, service: &AppService) -> anyhow::Result<()> {
    let out = service
        .multi_get(&MultiGetRequest {
            pattern: args.pattern,
            max_lines: args.max_lines,
            max_bytes: args.max_bytes,
            line_numbers: args.line_numbers,
        })
        .map_err(anyhow::Error::msg)?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        for err in out.errors {
            eprintln!("warning: {err}");
        }
        for doc in out.documents {
            println!("# {} ({})", doc.name, doc.uri);
            println!("{}", doc.text);
            println!();
        }
    }
    Ok(())
}

fn handle_ls(args: ListArgs, service: &AppService) -> anyhow::Result<()> {
    let files = service
        .list_files(args.prefix.as_deref())
        .map_err(anyhow::Error::msg)?;
    for file in files {
        println!("{file}");
    }
    Ok(())
}

fn handle_collection(args: CollectionArgs, service: &AppService) -> anyhow::Result<()> {
    match args.subcommand {
        CollectionSubcommand::List { json } => {
            let collections = service.list_collections().map_err(anyhow::Error::msg)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&collections)?);
            } else {
                for c in collections {
                    println!("{}\t{}\t{}\t{}", c.name, c.path, c.pattern, c.documents);
                }
            }
        }
        CollectionSubcommand::Add { path, mask, name } => {
            let canonical = path
                .canonicalize()
                .with_context(|| format!("collection path does not exist: {}", path.display()))?;
            let inferred = name.unwrap_or_else(|| {
                canonical
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("collection")
                    .to_string()
            });
            service
                .add_collection(&CollectionMutation {
                    name: inferred.clone(),
                    path: canonical.to_string_lossy().to_string(),
                    pattern: mask,
                })
                .map_err(anyhow::Error::msg)?;
            println!("added collection {inferred}");
        }
        CollectionSubcommand::Remove { name } => {
            let removed = service
                .remove_collection(&name)
                .map_err(anyhow::Error::msg)?;
            if removed {
                println!("removed collection {name}");
            } else {
                println!("collection not found: {name}");
            }
        }
        CollectionSubcommand::Rename { old_name, new_name } => {
            service
                .rename_collection(&old_name, &new_name)
                .map_err(anyhow::Error::msg)?;
            println!("renamed collection {old_name} -> {new_name}");
        }
    }
    Ok(())
}

fn handle_status(args: StatusArgs, service: &AppService) -> anyhow::Result<()> {
    let status = service.status().map_err(anyhow::Error::msg)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&status)?);
    } else {
        println!("total_documents: {}", status.total_documents);
        println!("needs_embedding: {}", status.needs_embedding);
        println!("has_vector_index: {}", status.has_vector_index);
        println!("collections:");
        for collection in status.collections {
            println!(
                "  - {} [{} docs] {}",
                collection.name, collection.documents, collection.path
            );
        }
    }
    Ok(())
}

fn handle_update(service: &AppService) -> anyhow::Result<()> {
    let updates = service.run_updates().map_err(anyhow::Error::msg)?;
    if updates.is_empty() {
        println!("no collection update hooks configured; continuing to ingest");
    }

    let mut has_failures = false;
    for item in updates {
        println!(
            "{}: {} (exit={:?})",
            item.collection, item.success, item.exit_code
        );
        if !item.success {
            has_failures = true;
            if !item.stderr.is_empty() {
                eprintln!("{}", item.stderr);
            }
        }
    }

    if has_failures {
        bail!("one or more update hooks failed");
    }
    handle_ingest(false, service)
}

fn handle_ingest(force: bool, service: &AppService) -> anyhow::Result<()> {
    let report = service.ingest(force).map_err(anyhow::Error::msg)?;
    println!("scanned_files: {}", report.scanned_files);
    println!("indexed_documents: {}", report.indexed_documents);
    println!("updated_documents: {}", report.updated_documents);
    println!("unchanged_documents: {}", report.unchanged_documents);
    println!("deactivated_documents: {}", report.deactivated_documents);
    println!("failed_files: {}", report.failed_files);
    println!("duration_ms: {}", report.duration_ms);
    Ok(())
}

fn handle_embed(force: bool, service: &AppService) -> anyhow::Result<()> {
    let report = service.embed(force).map_err(anyhow::Error::msg)?;
    println!("model: {}", report.model);
    println!("dimension: {}", report.dimension);
    println!("embedded: {}", report.embedded);
    println!("skipped: {}", report.skipped);
    println!("failed: {}", report.failed);
    println!("duration_ms: {}", report.duration_ms);
    Ok(())
}

fn handle_pull(refresh: bool, service: &AppService) -> anyhow::Result<()> {
    let inventory = service.local_models().map_err(anyhow::Error::msg)?;
    println!("local model inventory:");
    for m in &inventory {
        println!(
            "- {} available={} {}",
            m.name,
            m.available,
            m.location.clone().unwrap_or_default()
        );
    }

    let pulled = service.pull_models(refresh).map_err(anyhow::Error::msg)?;
    println!("pull results:");
    for r in pulled {
        println!("- {} success={} {}", r.name, r.success, r.note);
    }
    Ok(())
}

fn handle_context(args: ContextArgs, service: &AppService) -> anyhow::Result<()> {
    match args.subcommand {
        ContextSubcommand::Add { args } => {
            let (raw_target, text) = parse_context_add_args(&args)?;
            let collections = service.list_collections().map_err(anyhow::Error::msg)?;
            let target =
                resolve_context_target(raw_target.as_deref(), &collections, &env::current_dir()?)?;
            service
                .add_context(&ContextMutation { target, text })
                .map_err(anyhow::Error::msg)?;
            println!("context added");
        }
        ContextSubcommand::List { json } => {
            let list = service.list_contexts().map_err(anyhow::Error::msg)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&list)?);
            } else {
                print_context_list(&list);
            }
        }
        ContextSubcommand::Check => {
            let status = service.status().map_err(anyhow::Error::msg)?;
            let contexts = service.list_contexts().map_err(anyhow::Error::msg)?;
            if contexts.is_empty() {
                println!("no contexts configured");
                return Ok(());
            }
            let mut files_by_collection = BTreeMap::<String, Vec<String>>::new();
            for collection in &status.collections {
                let prefix = format!("{}/", collection.name);
                let files = service
                    .list_files(Some(&prefix))
                    .map_err(anyhow::Error::msg)?
                    .into_iter()
                    .filter_map(|value| {
                        value
                            .strip_prefix(&prefix)
                            .map(std::string::ToString::to_string)
                    })
                    .collect::<Vec<_>>();
                files_by_collection.insert(collection.name.clone(), files);
            }

            let report =
                build_context_gap_report(&status.collections, &contexts, &files_by_collection);
            print_context_gap_report(&report);
        }
        ContextSubcommand::Rm { target } => {
            let collections = service.list_collections().map_err(anyhow::Error::msg)?;
            let target = resolve_context_target(Some(&target), &collections, &env::current_dir()?)?;
            let removed = service
                .remove_context(&target)
                .map_err(anyhow::Error::msg)?;
            if removed {
                println!("context removed");
            } else {
                println!("context not found");
            }
        }
    }
    Ok(())
}

fn handle_cleanup(json: bool, service: &AppService) -> anyhow::Result<()> {
    let report = service.cleanup().map_err(anyhow::Error::msg)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("deleted llm cache rows: {}", report.llm_cache_rows);
        println!("deleted inactive documents: {}", report.inactive_documents);
        println!("deleted orphaned content rows: {}", report.orphaned_content);
        println!("deleted orphaned vector rows: {}", report.orphaned_vectors);
    }
    Ok(())
}

fn parse_context_target(input: &str) -> anyhow::Result<ContextTarget> {
    let value = input.trim();
    if value == "/" {
        return Ok(ContextTarget::Global);
    }

    let normalized = value
        .strip_prefix("qmd://")
        .map_or_else(|| value.to_string(), |v| v.to_string());

    let mut parts = normalized.splitn(2, '/');
    let collection = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();
    if collection.is_empty() {
        bail!("invalid context target '{input}', expected '/' or 'collection/path'");
    }

    let normalized_path = if path.is_empty() { "/" } else { path };
    Ok(ContextTarget::CollectionPath {
        collection: collection.to_string(),
        path: normalized_path.to_string(),
    })
}

fn parse_context_add_args(args: &[String]) -> anyhow::Result<(Option<String>, String)> {
    match args {
        [text] => Ok((None, text.clone())),
        [target, text] => Ok((Some(target.clone()), text.clone())),
        _ => bail!("context add requires either '<text>' or '<target> <text>'"),
    }
}

fn resolve_context_target(
    input: Option<&str>,
    collections: &[CollectionRecord],
    cwd: &Path,
) -> anyhow::Result<ContextTarget> {
    let raw = input.unwrap_or(".").trim();
    if raw == "/" {
        return Ok(ContextTarget::Global);
    }

    if is_explicit_virtual_target(raw, collections) {
        return parse_context_target(raw);
    }

    let fs_path = resolve_filesystem_path(raw, cwd)?;
    if let Some((collection, relative)) = detect_collection_from_path(&fs_path, collections) {
        let normalized = normalize_context_path(&relative);
        return Ok(ContextTarget::CollectionPath {
            collection,
            path: normalized,
        });
    }

    bail!(
        "path is not in any indexed collection: {}",
        fs_path.display()
    )
}

fn is_explicit_virtual_target(raw: &str, collections: &[CollectionRecord]) -> bool {
    if raw.starts_with("qmd://") {
        return true;
    }
    if raw.starts_with("./") || raw.starts_with("../") || raw.starts_with("~/") {
        return false;
    }
    if Path::new(raw).is_absolute() {
        return false;
    }
    let first = raw.split('/').next().unwrap_or_default();
    collections.iter().any(|c| c.name == first)
}

fn resolve_filesystem_path(raw: &str, cwd: &Path) -> anyhow::Result<PathBuf> {
    if raw == "." || raw == "./" {
        return Ok(cwd.to_path_buf());
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Ok(home) = env::var("HOME").or_else(|_| env::var("USERPROFILE")) {
            return Ok(PathBuf::from(home).join(rest));
        }
    }

    let candidate = PathBuf::from(raw);
    if candidate.is_absolute() {
        Ok(candidate)
    } else {
        Ok(cwd.join(candidate))
    }
}

fn detect_collection_from_path(
    fs_path: &Path,
    collections: &[CollectionRecord],
) -> Option<(String, String)> {
    let fs_norm = normalize_fs_path(fs_path);
    let mut best: Option<(usize, String, String)> = None;

    for collection in collections {
        let root_norm = normalize_fs_path(Path::new(&collection.path));
        if !path_prefix_matches(&fs_norm, &root_norm) {
            continue;
        }

        let rel = if fs_norm.len() == root_norm.len() {
            "/".to_string()
        } else {
            fs_norm[root_norm.len() + 1..].to_string()
        };

        let should_replace = best
            .as_ref()
            .map(|(len, _, _)| root_norm.len() > *len)
            .unwrap_or(true);
        if should_replace {
            best = Some((root_norm.len(), collection.name.clone(), rel));
        }
    }

    best.map(|(_, name, rel)| (name, rel))
}

fn path_prefix_matches(path: &str, root: &str) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .map(|suffix| suffix.starts_with('/'))
            .unwrap_or(false)
}

fn normalize_fs_path(path: &Path) -> String {
    let normalized = path
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_string();
    if cfg!(windows) {
        normalized.to_ascii_lowercase()
    } else {
        normalized
    }
}

fn normalize_context_path(path: &str) -> String {
    let normalized = path.replace('\\', "/").trim().trim_matches('/').to_string();
    if normalized.is_empty() {
        "/".to_string()
    } else {
        normalized
    }
}

fn print_context_list(list: &[ContextRecord]) {
    if list.is_empty() {
        println!("no contexts configured");
        return;
    }

    let mut grouped = BTreeMap::<String, Vec<&ContextRecord>>::new();
    for entry in list {
        grouped.entry(entry.scope.clone()).or_default().push(entry);
    }

    for (scope, items) in grouped {
        if scope == "global" {
            println!("global");
        } else {
            println!("{scope} (qmd://{scope}/)");
        }
        for entry in items {
            let preview = preview_context_text(&entry.text, 80);
            println!("  {} -> {}", entry.path, preview);
        }
    }
}

fn preview_context_text(text: &str, limit: usize) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= limit {
        compact
    } else {
        let mut out = String::new();
        for ch in compact.chars().take(limit.saturating_sub(3)) {
            out.push(ch);
        }
        out.push_str("...");
        out
    }
}

#[derive(Debug, Default)]
struct ContextGapReport {
    missing_collections: Vec<String>,
    missing_top_paths: BTreeMap<String, Vec<String>>,
}

fn build_context_gap_report(
    collections: &[CollectionRecord],
    contexts: &[ContextRecord],
    files_by_collection: &BTreeMap<String, Vec<String>>,
) -> ContextGapReport {
    let has_global = contexts.iter().any(|ctx| ctx.scope == "global");
    let mut report = ContextGapReport::default();

    for collection in collections {
        if collection.documents == 0 {
            continue;
        }

        let collection_paths = contexts
            .iter()
            .filter(|ctx| ctx.scope == collection.name)
            .map(|ctx| normalize_context_path(&ctx.path))
            .collect::<Vec<_>>();

        let has_collection_context = has_global || !collection_paths.is_empty();
        if !has_collection_context {
            report.missing_collections.push(collection.name.clone());
            continue;
        }
        if has_global || collection_paths.iter().any(|path| path == "/") {
            continue;
        }

        let top_paths = files_by_collection
            .get(&collection.name)
            .into_iter()
            .flat_map(|files| files.iter())
            .filter_map(|file| file.split('/').next())
            .filter(|segment| !segment.is_empty())
            .map(str::to_string)
            .collect::<BTreeSet<_>>();

        let mut missing = Vec::new();
        for top in top_paths {
            if !context_paths_cover_top_path(&top, &collection_paths) {
                missing.push(top);
            }
        }
        if !missing.is_empty() {
            report
                .missing_top_paths
                .insert(collection.name.clone(), missing);
        }
    }

    report
}

fn context_paths_cover_top_path(top_path: &str, context_paths: &[String]) -> bool {
    context_paths.iter().any(|context_path| {
        context_path == "/"
            || context_path == top_path
            || context_path.starts_with(&(top_path.to_string() + "/"))
            || top_path.starts_with(&(context_path.to_string() + "/"))
    })
}

fn print_context_gap_report(report: &ContextGapReport) {
    if report.missing_collections.is_empty() && report.missing_top_paths.is_empty() {
        println!("all collections and top-level paths have context coverage");
        return;
    }

    if !report.missing_collections.is_empty() {
        println!("collections without context:");
        for collection in &report.missing_collections {
            println!("  - {collection}");
            println!(
                "    suggestion: qmd context add qmd://{collection}/ \"Description of {collection}\""
            );
        }
    }

    if !report.missing_top_paths.is_empty() {
        println!("top-level directories without context:");
        for (collection, paths) in &report.missing_top_paths {
            println!("  {collection}:");
            for path in paths {
                println!("    - {path}");
                println!(
                    "      suggestion: qmd context add qmd://{collection}/{path} \"Description of {path}\""
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collection(name: &str, path: &str) -> CollectionRecord {
        CollectionRecord {
            name: name.to_string(),
            path: path.to_string(),
            pattern: "**/*.md".to_string(),
            documents: 1,
            last_updated: None,
        }
    }

    #[test]
    fn parse_context_add_args_defaults_target_to_current_dir() {
        let (target, text) = parse_context_add_args(&["project summary".to_string()]).unwrap();
        assert_eq!(target, None);
        assert_eq!(text, "project summary");
    }

    #[test]
    fn resolve_context_target_prefers_longest_collection_prefix() {
        let collections = vec![
            collection("repo", "/workspace"),
            collection("docs", "/workspace/docs"),
        ];

        let target = resolve_context_target(
            Some("/workspace/docs/guides/setup.md"),
            &collections,
            Path::new("/workspace"),
        )
        .unwrap();

        match target {
            ContextTarget::CollectionPath { collection, path } => {
                assert_eq!(collection, "docs");
                assert_eq!(path, "guides/setup.md");
            }
            ContextTarget::Global => panic!("expected collection path"),
        }
    }

    #[test]
    fn build_context_gap_report_detects_missing_top_level_paths() {
        let collections = vec![CollectionRecord {
            name: "docs".to_string(),
            path: "/repo/docs".to_string(),
            pattern: "**/*.md".to_string(),
            documents: 3,
            last_updated: None,
        }];
        let contexts = vec![ContextRecord {
            scope: "docs".to_string(),
            path: "api".to_string(),
            text: "API docs".to_string(),
        }];
        let mut files = BTreeMap::new();
        files.insert(
            "docs".to_string(),
            vec![
                "api/http.md".to_string(),
                "guides/setup.md".to_string(),
                "guides/ops.md".to_string(),
            ],
        );

        let report = build_context_gap_report(&collections, &contexts, &files);
        assert!(report.missing_collections.is_empty());
        assert_eq!(
            report.missing_top_paths.get("docs"),
            Some(&vec!["guides".to_string()])
        );
    }
}
