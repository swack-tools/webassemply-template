# Google Antigravity routing and workspace rules

This file configures **Google Antigravity Desktop (Antigravity 2.0)** and **Antigravity IDE** when working in this repository. Antigravity automatically discovers and loads both `GEMINI.md` and `AGENTS.md` from the repository root, as well as workspace rules under `.agents/rules/`. Read [AGENTS.md](AGENTS.md) first for repository-wide Cloudflare Workers `KV` invariants, security boundaries, and Git review gates.

## Antigravity model routing modules

Keep the human-selected main-session model (such as Gemini 4 Argon at High effort) unchanged for main-loop orchestration, planning, and final synthesis. When delegating bounded tasks via `invoke_subagent`, the Gemini command-line tool, or the Antigravity and Google GenAI SDKs, route each task to one of the four modules below based on failure impact, state interaction, and ambiguity.

### Module 1 for heavy complexity, architecture, and verification (`Gemini 4 Argon` at `High` effort)

- **Model**: `Gemini 4 Argon` (`gemini-4-argon`)
- **Effort (`thinking_level`)**: `high` (High)
- **Antigravity `invoke_subagent` tier**: `Model: "pro"` (or `Model: "inherit"` when the parent session already runs Gemini 4 Argon at High effort)
- **Target workloads**:
  - Architecture and decomposition decisions across the Rust Cloudflare Worker (`cloudflare/src/lib.rs`) and `KV` state layer (`cloudflare/src/state.rs`).
  - Consolidated `KV` document contracts (`StateBundle`), monotonic `state_version` guards, canonical `SHA-256` `state_fingerprint` diff-gating, and distributed `KV` lease expiration logic.
  - Cryptographic authentication and security boundaries (`HMAC-SHA256` session cookies, `SESSION_NOT_BEFORE` cut-offs, OAuth 2.0 PKCE flows, and path normalization).
  - Timing-flake and concurrency race diagnosis across Wasm `thread_local!` caches and asynchronous `KV` writes.
  - Final pre-pull-request verification, judgment, and synthesis stages (`verify`, `judge`, and `synthesize`).

### Module 2 for cross-module engineering and deep debugging (`Gemini 3.8 Flash` at `High` effort)

- **Model**: `Gemini 3.8 Flash` (`gemini-3.8-flash`)
- **Effort (`thinking_level`)**: `high` (High)
- **Antigravity `invoke_subagent` tier**: `Model: "flash"`
- **Target workloads**:
  - Cross-module debugging spanning `lib.rs`, `state.rs`, `auth.rs`, `reconciler.rs`, and `ui.rs`.
  - Implementing substantive Worker endpoints, scheduled cron reconciliation transitions, and pattern-following multi-file refactors from a decided design.
  - Authoring concurrency, lease-contention, and adversarial `KV` quota-conservation test suites in `cloudflare/tests/`.
  - Code review finder passes where state interactions span multiple modules.

### Module 3 for bounded implementation, unit tests, and documentation (`Gemini 3.8 Flash` at `Medium` effort)

- **Model**: `Gemini 3.8 Flash` (`gemini-3.8-flash`)
- **Effort (`thinking_level`)**: `medium` (Medium)
- **Antigravity `invoke_subagent` tier**: `Model: "flash"`
- **Target workloads**:
  - Bounded feature implementation or bug fixes with explicit contracts and low cross-module ambiguity.
  - Adding focused unit tests and test scaffolding in `cloudflare/tests/`.
  - Writing and updating Markdown documentation in `docs/` to comply with Vale and the Google developer documentation style guide.
  - Mechanical migrations such as symbol renames or applying a decided pattern across files.

### Module 4 for fast discovery, symbol inventory, and grep fan-out (`Gemini 3.5 Flash-Lite` at `High` effort)

- **Model**: `Gemini 3.5 Flash-Lite` (`gemini-3.5-flash-lite`)
- **Effort (`thinking_level`)**: `high` (High)
- **Antigravity `invoke_subagent` tier**: `Model: "flash_lite"`
- **Target workloads**:
  - High-volume read-only exploration fan-out, basic code identification, and grepping scenarios.
  - Fast file and symbol inventory across the workspace.
  - Summarizing build logs, test output, or pull request comments.
  - Read-only formatting, configuration, and `.vale.ini` registration checks.

## Cross-harness routing comparison (all three harnesses)

Use this unified matrix to keep task routing consistent across **Google Antigravity Desktop and IDE**, **Claude Desktop and Claude Code**, and **ChatGPT and OpenAI Codex Desktop**:

| Task complexity tier | Google Antigravity (`GEMINI.md`) | Claude Desktop and Code (`CLAUDE.md`) | ChatGPT and Codex Desktop (`AGENTS.md`) | Typical Cloudflare Worker + `KV` tasks |
| --- | --- | --- | --- | --- |
| **Tier 1 (fast discovery and bulk fan-out)** | `Gemini 3.5 Flash-Lite` (`gemini-3.5-flash-lite`, `flash_lite`) at `High` effort | `Haiku 5.5` (`haiku-5.5`, `haiku`) at `High` effort | `gpt-6-luna` at `high` effort | Grep-and-report lookups, symbol/file inventory, log filtering, and config checks |
| **Tier 2 (bounded coding and docs)** | `Gemini 3.8 Flash` (`gemini-3.8-flash`, `flash`) at `Medium` effort | `Sonnet 5.5` (`sonnet-5.5`, `sonnet`) at `Medium` effort | `gpt-6.1-sol` at `medium` effort | Scoped endpoints, unit test scaffolding, mechanical refactors, and Vale docs updates |
| **Tier 3 (cross-module and concurrency)** | `Gemini 3.8 Flash` (`gemini-3.8-flash`, `flash`) at `High` effort | `Sonnet 5.5` (`sonnet-5.5`, `sonnet` / `opus`) at `High` effort | `gpt-6.1-sol` at `high` effort | Multi-file refactors, `KV` lease debugging, reconciler state tracing, and PR diff reviews |
| **Tier 4 (heavy complexity and judge)** | `Gemini 4 Argon` (`gemini-4-argon`, `pro` / `inherit`) at `High` effort | `Opus 5.5` (`opus-5.5`, `fable` / `opus`) at `High` effort | `gpt-6-astra` at `high` effort | Architecture design, `KV` consistency/fingerprint contracts, `HMAC` auth security, and final verification |

## Antigravity subagent and workspace orchestration

When delegating tasks in Antigravity Desktop (2.0) or Antigravity IDE:

1. **Choose the right subagent type**:
   - Use `TypeName: "research"` for read-only codebase exploration, symbol lookups, and documentation surveys.
   - Use `TypeName: "self"` for subagents that edit files, run `cargo test`, execute `worker-build --release`, or run `vale`.
   - Use `define_subagent` when creating a specialized, reusable role (such as an independent `KV` quota challenger or Vale style auditor) that you invoke multiple times during a session.
2. **Select the workspace isolation mode (`Workspace`)**:
   - `Workspace: "inherit"`: use for read-only `research` subagents or a single writing subagent operating on disjoint file paths.
   - `Workspace: "share"`: use when launching multiple concurrent `self` writing subagents. Each subagent gets an independent working tree backed by the shared Git repository directory without duplicating storage.
   - `Workspace: "branch"`: use when a subagent requires a fully cloned sandbox workspace for experimental or high-risk refactoring.
3. **Rely on reactive wakeup (never poll)**:
   - Antigravity automatically resumes your turn when a subagent sends a message or a background task completes. Never run shell `sleep` loops or poll `manage_subagents` / `manage_task` repeatedly.
4. **Enforce model hygiene**:
   - When calling the Gemini API or SDKs directly, always set `thinking_level` (`low`, `medium`, or `high`) explicitly and use current Gemini 3.5, 3.8, or 4 models. Never use deprecated `gemini-2.5-*`, `gemini-2.0-*`, or `gemini-1.5-*` models.

## Antigravity IDE and Desktop modality routing

Select the appropriate Antigravity surface for the scope of your change:

- **Antigravity Tab and Supercomplete (IDE)**: use for single-line completions, next-edit cursor jumps, and automatic `use` imports while editing Rust modules.
- **Inline Command (`Cmd+I` / `Ctrl+I` in IDE)**: use for localized edits within a highlighted function, adding docstrings, or refactoring a single struct in `cloudflare/src/state.rs`.
- **Sidebar Chat and Agent Canvas (IDE and Desktop 2.0)**: use for multi-file features, running `cargo test` and `vale`, and orchestrating subagents.
- **Recommended slash commands**:
  - `/plan`: structure a step-by-step implementation blueprint before modifying `KV` state schemas or auth flows.
  - `/boost`: activate deep multi-perspective reasoning and rigorous verification for complex Rust/Wasm debugging.
  - `/teamwork-preview`: coordinate parallel explorer, worker, and challenger subagents across isolated workspaces on large milestones.
  - `/grill-me`: interactively resolve ambiguous design trade-offs before writing code.
