# Codex Config Alignment (2026-02-24)

## Scope

Aligned `C:\Users\david\.codex\config.toml` and MCP launch scripts to current Codex guidance while moving QMD to repo-file-driven configuration.

## Sources used

- https://developers.openai.com/codex/config
- https://developers.openai.com/codex/config-reference
- https://developers.openai.com/codex/config-advanced
- https://developers.openai.com/codex/config-examples
- https://raw.githubusercontent.com/openai/codex/main/docs/config.md

## Alignment changes applied

1. Kept documented MCP server fields (`enabled`, `required`, `command`, `args`, `cwd`, timeouts).
2. Kept QMD `enabled_tools` scoping.
3. Switched QMD runtime to file-first config model:
   - repo `qmd.config.json`
   - repo `.env`
   - environment variables only as fallback/pass-through
4. Updated QMD launcher to Rust-first default behavior.
5. Removed QMD `env_vars` passthrough for DB fields to avoid env-first behavior and keep repo config authoritative.
6. Increased QMD MCP timeouts (`startup_timeout_sec = 180`, `tool_timeout_sec = 300`) to avoid cold-start/DB-bootstrap timeout failures.
7. Removed unnecessary hardcoded QMD tuning/env values from `.codex/config.toml` where repo config now owns them.

## Validation

- `run-qmd-mcp.ps1 -Check` passes and shows `prefer_rust_default: True`.
- `run-cloudflare-workers.ps1 -Check` passes.
- `run-qmd-mcp.ps1 -Check` continues to resolve DSN from repo config even when conflicting shell PG env vars are set.
