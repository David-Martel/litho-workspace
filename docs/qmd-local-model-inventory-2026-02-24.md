# QMD Local Model Inventory (2026-02-24)

## Candidate model locations scanned

- `C:\Users\david\.ollama`
- `C:\codedev`
- `T:\projects`
- `C:\Users\david\.cache\qmd\models`

## Best usable local models for QMD now

### Already available in QMD cache (direct match for QMD TS defaults)

- `C:\Users\david\.cache\qmd\models\hf_ggml-org_embeddinggemma-300M-Q8_0.gguf` (~0.31 GB)
- `C:\Users\david\.cache\qmd\models\hf_ggml-org_qwen3-reranker-0.6b-q8_0.gguf` (~0.60 GB)
- `C:\Users\david\.cache\qmd\models\hf_tobil_qmd-query-expansion-1.7B-q4_k_m.gguf` (~1.19 GB)

These map directly to:
- embed model URI
- reranker model URI
- query-expansion model URI

No additional download is required for the default QMD local pipeline.

### Ollama models available (alternative local fallback)

From `ollama list`:
- `nomic-embed-text:latest` (274 MB)
- `embedding-gemma-2b:latest` (274 MB)
- `qwen2.5-coder:3b` (1.9 GB)
- `qwen2.5-coder:7b` (4.7 GB)
- `llama3.1:latest` (4.9 GB)
- `mistral:latest` (4.4 GB)
- `deepseek-r1:7b/8b`
- `gemma3:4b/12b`
- `gpt-oss:20b`

These are usable if/when a Rust LLM backend adds an Ollama provider adapter.

### Additional GGUF found outside Ollama

- `T:\projects\archive\rust-v7.0.0-copy-20260125\integrations\rust-llm\models\deepseek-coder-1.3b-base.Q4_K_M.gguf` (~0.81 GB)

## Recommendation

1. Keep using QMD's existing cached GGUF trio for parity and deterministic behavior.
2. In Rust phase 2, add optional provider selection:
   - `node-llama-cpp GGUF cache` compatible provider for strict parity.
   - `Ollama` provider for operational flexibility.
3. Use embed/rerank/query model overrides in config rather than hardcoding paths.
