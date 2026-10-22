# AI harnesses and model routing

This template repository is pre-configured to work seamlessly across three AI development environments:

1. **Claude Desktop and Claude Code** (`CLAUDE.md` and `AGENTS.md`)
2. **ChatGPT and OpenAI Codex Desktop and command-line tool** (`AGENTS.md`)
3. **Google Antigravity Desktop (Antigravity 2.0) and Antigravity IDE** (`GEMINI.md`, `.agents/rules/routing.md`, and `AGENTS.md`)

## How each harness loads instructions

| Harness | Discovery mechanism | Primary files read |
| --- | --- | --- |
| **Claude Desktop and Claude Code** | Reads `CLAUDE.md` at the repository root. Line 1 of `CLAUDE.md` contains `@read AGENTS.md` (and `@AGENTS.md`) so Claude loads shared engineering rules before applying its subagent routing policy. | `CLAUDE.md`, `AGENTS.md` |
| **ChatGPT and OpenAI Codex Desktop** | Walks up from the working directory to the repository root and loads `AGENTS.md`. | `AGENTS.md` |
| **Google Antigravity Desktop and IDE** | Walks up to the repository root to load `GEMINI.md` and `AGENTS.md`, and scans `.agents/rules/*.md` for workspace-scoped rules. | `GEMINI.md`, `AGENTS.md`, `.agents/rules/routing.md` |

## Unified 4-tier model routing system

Across all three harnesses, keep the human-selected main-session model unchanged for top-level orchestration, judgment calls, and final synthesis. When spawning subagents for parallel exploration, implementation, or verification, route by **failure impact**, **state interaction**, and **ambiguity** rather than diff line count:

| Tier | Workload profile | Google Antigravity (`GEMINI.md`) | Claude Desktop and Code (`CLAUDE.md`) | ChatGPT and Codex Desktop (`AGENTS.md`) |
| --- | --- | --- | --- | --- |
| **Tier 1 (bulk discovery and grep fan-out)** | Per-file classification, grep-and-report symbol lookups, file inventory, and read-only formatting or configuration checks. | `Gemini 3.5 Flash-Lite` (`gemini-3.5-flash-lite`, `flash_lite`) at `High` effort | `Haiku 5.5` (`haiku-5.5`, tier alias `haiku`) at `High` effort | `gpt-6-luna` at `high` effort |
| **Tier 2 (mechanical and bounded coding)** | Exploration fan-out, mechanical migrations, log/PR summaries, unit test scaffolding, bounded implementation, and Vale docs updates. | `Gemini 3.8 Flash` (`gemini-3.8-flash`, `flash`) at `Medium` effort | `Sonnet 5.5` (`sonnet-5.5`, tier alias `sonnet`) at `Medium` effort | `gpt-6.1-sol` at `medium` effort |
| **Tier 3 (substantive and cross-module code)** | Implementing features with a decided design, cross-module debugging across `lib.rs` and `state.rs`, `KV` lease reasoning, review finder passes, and nontrivial tests. | `Gemini 3.8 Flash` (`gemini-3.8-flash`, `flash`) at `High` effort | `Sonnet 5.5` (`sonnet-5.5`, tier alias `opus` / `sonnet`) at `High` effort | `gpt-6.1-sol` at `high` effort |
| **Tier 4 (heavy complexity and judge stages)** | Architecture/decomposition decisions, `KV` fingerprint and version contracts, `HMAC-SHA256` security triage, race debugging, and workflow `verify`/`judge`/`synthesize` stages. | `Gemini 4 Argon` (`gemini-4-argon`, `pro` / `inherit`) at `High` effort | `Opus 5.5` (`opus-5.5`, tier alias `fable` / `opus`) at `High` effort | `gpt-6-astra` at `high` effort |

## Claude routing rules

`CLAUDE.md` begins with `@read AGENTS.md` and defines the Claude-specific subagent routing policy:

- **Workflow stages**: route `finders` and `sweepers` to `sonnet` (`sonnet-5.5` at `medium` effort, or `high` / `opus-5.5` when the code is subtle). Route `verify`, `judge`, and `synthesize` stages to `fable` (`opus-5.5` at `high` effort). Never run a stage with more than 5 concurrent agents on `fable` (`opus-5.5`) unless it is a `verify` or `judge` stage.
- **Prefer agents as the routing mechanism**: because the session model cannot switch mid-run, spawn a subagent with an explicit `model` and `effort` whenever a unit of work is self-contained enough to hand off (exploration, migrations, review passes, per-file sweeps, or verification). Keep inline only work that depends on accumulated conversation context or edits that are too small to justify a handoff.
- **Mismatch reporting**: if the active session model is mismatched to the task tier or an environment variable such as `CLAUDE_CODE_EFFORT_LEVEL` overrides per-launch parameters, state the mismatch and suggest `/model`. Never proceed silently on a noticed mismatch.

## Antigravity routing rules

`GEMINI.md` and `.agents/rules/routing.md` configure subagent orchestration and workspace isolation for Antigravity:

- **Subagent invocation (`invoke_subagent`)**:
  - Pass `Model: "pro"` (or `"inherit"` when the parent session runs Gemini 4 Argon at High effort) for Tier 4 architecture, security, and verification tasks.
  - Pass `Model: "flash"` for Tier 2 and Tier 3 implementation, refactoring, and test tasks (`Gemini 3.8 Flash`).
  - Pass `Model: "flash_lite"` for Tier 1 read-only symbol lookup and grep fan-out (`Gemini 3.5 Flash-Lite`).
- **Workspace isolation (`Workspace`)**:
  - Use `Workspace: "inherit"` for read-only `research` subagents.
  - Use `Workspace: "share"` for concurrent `self` writing subagents so each writer works in an isolated working tree backed by the shared Git object store.
  - Use `Workspace: "branch"` for sandboxed experimental branches.
- **Reactive wakeup**: the Antigravity runtime wakes the parent agent automatically when a subagent sends a message or a background task finishes. Never poll subagent status in a loop.
