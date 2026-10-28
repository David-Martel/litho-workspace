# QMD MCP request parameters

The MCP binary imports its service and request types from
[`litho-qmd-core`](../crates/litho-qmd-core/src/lib.rs). The concrete SQLite and
PostgreSQL stores remain in
[`litho-qmd-storage`](../crates/litho-qmd-storage/src/lib.rs).
This keeps the binary compatible with the published crate dependency direction.

[`main.rs`](../crates/litho-qmd-mcp/src/main.rs) validates the `tools/call`,
`resources/read` and `prompts/get` parameter objects. Each accepts optional
protocol-level `_meta` as an object. Its contents remain opaque to the QMD
operation, and it is never forwarded into tool arguments. Required parameters,
unknown-field rejection and individual tool argument schemas still apply.

The two `protocol_metadata_*` tests in
[`request_contract_tests.rs`](../crates/litho-qmd-mcp/src/request_contract_tests.rs)
check accepted metadata on all three request schemas and reject scalar metadata
and reserved metadata supplied as a tool argument. Their recorded review belongs in the root
[`TEST_REVIEW.csv`](../TEST_REVIEW.csv); existing tests without a recorded review
remain unreviewed. The generated [scoped test inventory](../TEST_INVENTORY.md) uses
the [scoped checker configuration](qmd-test-review.toml). This advisory inventory
covers this crate, the private startup-delivery controls and selected compaction,
skill-loader, PowerShell, user-agent, MCP and status fixture files used by the prerequisites in the
[build guide](../BUILD.md). Reviews belong to individual selectors; selecting a
file does not certify its untouched tests. Legacy rows without a recorded review
remain unreviewed and should be reviewed when their behavior next changes.
Other workspace and vendored tests are outside this advisory inventory.
The current shared collector does not enumerate `cfg_attr`-wrapped Tokio tests.
Three such compaction cases remain outside the discovered selectors and
have no recorded review; this inventory does not establish complete Rust test
enumeration.

With Python 3.11+ and uv, generate or verify the advisory inventory through the
[repository adapter](../scripts/qmd-test-review.py), pointing it at an
authenticated `vigil-utils` checkout containing the canonical shared checker:

```sh
uv run --no-project python scripts/qmd-test-review.py --checker-root ../vigil-utils
uv run --no-project python scripts/qmd-test-review.py --checker-root ../vigil-utils --check
```

The adapter uses the shared collector, hash, rationale and review classifier APIs.
It adds the selected compatibility files because the shared CLI accepts only directory
inputs. `--sync-register` appends newly discovered selectors as unreviewed; it
does not create reviews. Legacy unreviewed rows remain advisory debt.

Run the complete MCP unit suite with:

```sh
cargo test --locked -p litho-qmd-mcp
```

The repository's [build guide](../BUILD.md) and
[hook configuration](../lefthook.yml) define the broader format, Clippy and test
gates. A passing MCP unit suite does not qualify live PostgreSQL, model services
or an installed agent configuration.
