# AI agent guidelines

This file defines repository-wide architecture invariants, Cloudflare Workers `KV` state contracts, security guardrails, verification workflows, and multi-harness routing rules for this pure-Rust Cloudflare Worker template repository. It is designed to work across **Claude Desktop and Claude Code**, **ChatGPT and OpenAI Codex Desktop**, and **Google Antigravity Desktop and Antigravity IDE**.

## Cross-harness configuration map

Every supported AI coding harness discovers instructions from a specific root or workspace file:

| Harness | Primary configuration files | Role in this repository |
| --- | --- | --- |
| All harnesses | `AGENTS.md` | Shared architecture invariants, Cloudflare `KV` state rules, security boundaries, test/lint gates, Git workflow, Codex routing, and the Claude routing guide at the end of this file. |
| Claude Desktop and Claude Code | `CLAUDE.md` | Imports `AGENTS.md` on line 1 (`@read AGENTS.md`) and enforces the Claude-specific subagent model routing policy (`fable` / `opus-5.5`, `sonnet-5.5`, and `haiku-5.5`). |
| Google Antigravity Desktop and IDE | `GEMINI.md`, `.agents/rules/routing.md` | Defines Antigravity subagent routing modules (`Gemini 4 Argon`, `Gemini 3.8 Flash`, and `Gemini 3.5 Flash-Lite`), workspace isolation modes, and IDE/Desktop workflows. |
| ChatGPT and OpenAI Codex Desktop | `AGENTS.md` | Uses the general engineering rules and the Codex subagent routing table (`gpt-6-astra`, `gpt-6.1-sol`, and `gpt-6-luna`) defined in this file. |

## Repository overview and architecture

This template repository provides a production-ready foundation for building 100% pure-Rust serverless edge apps on Cloudflare Workers (`workers-rs`, compiled to `wasm32-unknown-unknown`) that use **Cloudflare Workers `KV`** (`STATE_KV`) as durable state storage.

### Primary entry points

| Entry point | Purpose | Mutates `KV` state? |
| --- | --- | --- |
| `cloudflare/src/lib.rs` (`#[event(fetch)]`) | Edge HTTP router serving the embedded web user interface (`/`), health probe (`/api/health`), state snapshot (`/api/state`), item CRUD endpoints (`/api/items`), and manual reconciler trigger (`/api/reconcile/tick`). | Read-only on `GET`; `POST` and `DELETE` routes mutate state when authenticated. |
| `cloudflare/src/lib.rs` (`#[event(scheduled)]`) | 5-minute scheduled cron reconciler that acquires a `KV` lease, reconciles app state, and writes to `KV` only when the canonical `SHA-256` state fingerprint changes. | Writes only on state diff or lease transitions. |
| `cloudflare/src/state.rs` | Consolidated `KV` state engine (`StateBundle`), `thread_local!` Wasm isolate cache, `SHA-256` fingerprint diff-gating, distributed `KV` lease locks, and the `KvStoreBackend` trait. | Core state persistence module. |
| `cloudflare/src/auth.rs` | Constant-time `HMAC-SHA256` session cookie signing and verification (`__Host-cf_kv_session`), `SESSION_NOT_BEFORE` revocation checks, and bearer token validation. | Read-only cryptographic verification. |
| `cloudflare/src/reconciler.rs` | Idempotent background reconciliation state machine with lease fencing and zero-write no-op optimization. | Updates state via `save_state_if_changed`. |
| `cloudflare/src/ui.rs` | Embedded single-page HTML, CSS, and JavaScript dashboard served directly from Worker memory in under 15 milliseconds. | Read-only asset module. |

### Key directories and files

| Path | Responsibility |
| --- | --- |
| `cloudflare/` | Pure-Rust Cloudflare Worker crate (`cf-worker-kv-template`) compiled to `wasm32-unknown-unknown` for the edge and native targets for `cargo test`. |
| `cloudflare/tests/` | Pure-Rust unit, concurrency, lease-locking, auth, and `KV` quota conservation test suites using `InMemoryKvStore`. |
| `docs/` and `mkdocs.yml` | MkDocs Material documentation site covering `KV` state architecture, multi-harness AI routing, Vale Google style enforcement, GitHub Pages setup, and customizing GitHub Actions runners for `swack-tools`. |
| `.agents/rules/` | Workspace-scoped rules discovered automatically by Google Antigravity Desktop and Antigravity IDE. |
| `.github/workflows/` | GitHub Actions workflows for Rust test suites, Vale prose linting (`ci.yml`), and strict MkDocs deployment to GitHub Pages (`docs.yml`). |
| `.vale.ini` and `.vale/` | Vale prose linter configuration enforcing the Google developer documentation style guide across all Markdown files. |

## Cloudflare `KV` state storage invariants

Cloudflare Workers `KV` provides low-latency global reads, eventual consistency across edge locations, and strict daily write limits on Free Tier accounts (1,000 writes/day and 100,000 reads/day). Preserve the following invariants whenever modifying state logic:

1. **Single consolidated state document**:
   - Store related app state, configuration, administrator records, and telemetry inside one consolidated JSON envelope under key `app:state` (`KV_STATE_KEY`) with schema identifier `cf-kv-state-snapshot-v1` (`SNAPSHOT_SCHEMA`).
   - Avoid scattering state across dozens of fine-grained `KV` keys; a single consolidated document guarantees that hydrating or persisting app state consumes at most one `KV` operation.
2. **In-isolate Wasm memory cache (`thread_local!`)**:
   - Maintain an in-isolate memory cache (`STATE_CACHE` in `cloudflare/src/state.rs`) with a 60-second time to live (`STATE_CACHE_TTL_SECS`).
   - Read endpoints check `get_cached_state(now)` before calling `kv.get(...)`. Write operations immediately update the in-isolate cache via `set_cached_state(now, bundle)` so subsequent requests within the same isolate observe read-after-write consistency.
3. **Canonical `SHA-256` fingerprint diff-gating**:
   - Compute `state_fingerprint` as the hex-encoded `SHA-256` digest of the canonical serialized `AppState` payload (`compute_state_fingerprint`).
   - Use `save_state_if_changed` in scheduled cron handlers and idempotent endpoints so unchanged state performs **zero** `KV` writes (`WriteOutcome::SkippedUnchanged`).
4. **Monotonic versioning and distributed lease locking**:
   - Increment `state_version` monotonically on every persisted state change.
   - Guard scheduled reconciliation ticks and multi-step mutations with `acquire_lease` and `release_lease` under key `app:lease` (`KV_LEASE_KEY`) using a bounded TTL so overlapping cron invocations never clobber concurrent writes.
5. **Dual-target `KvStoreBackend` trait (Cloudflare Edge + `wasmCloud` + native tests)**:
   - Write Wasm components in Rust using `workers-rs` (`worker` crate) compiled to `wasm32-unknown-unknown` and bind Workers `KV` directly via `worker::kv::KvStore` (`impl KvStoreBackend for worker::kv::KvStore`).
   - Support `wasmCloud` on Kubernetes (`wasm32-wasip2` + `wasmcloud:keyvalue` / `wasi:keyvalue/store`) through `WasiKeyValueBucket` and `WasmCloudKvAdapter<B>`, and run native unit tests with `InMemoryKvStore` via `cargo test` so business logic works natively across both Cloudflare Workers and `wasmCloud`.

## Security and operational guardrails

- **No unauthorized live cloud mutations**: a prompt to write code, add tests, or update documentation does not authorize running `wrangler deploy`, mutating production `KV` namespaces, rotating Worker secrets, or provisioning cloud runner VMs without explicit user approval. Always verify changes with local unit tests (`cargo test`), dry-run builds (`worker-build --release`), and documentation checks unless the user explicitly requests a live deployment.
- **Session and token security**:
  - Sign and verify session tokens in `cloudflare/src/auth.rs` using `HMAC-SHA256` with purpose label `cf-kv-worker/session/v1` and constant-time comparison (`subtle::ConstantTimeEq`).
  - Enforce the `__Host-` cookie prefix (`Secure; HttpOnly; SameSite=Lax; Path=/`) and honor `SESSION_NOT_BEFORE` for instant global session revocation.
- **Input bounds and path normalization**:
  - Enforce `MAX_AUTH_BODY_BYTES` (16 KiB) on authentication routes and `MAX_API_BODY_BYTES` (256 KiB) on API mutation routes, returning HTTP 413 (`payload_too_large`) when exceeded.
  - Normalize percent-encoded paths, backslashes, and dot segments through `normalize_path_for_blocking` before routing, and reject `/api/control*` paths with HTTP 404.
- **Public documentation hygiene**:
  - Keep real account IDs, `KV` namespace IDs, OAuth 2.0 client secrets, session secrets, and private hostnames out of Git and out of `docs/`. Use generic placeholders such as `REPLACE_WITH_KV_NAMESPACE_ID` or `your-gcp-project-id`.

## Development, testing, and verification workflow

Run these verification checks before committing or opening a pull request:

1. **Pure-Rust unit, concurrency, and quota-conservation tests**:
   ```bash
   cargo test --manifest-path cloudflare/Cargo.toml
   ```
2. **Rust formatting and compiler checks**:
   ```bash
   cargo fmt --manifest-path cloudflare/Cargo.toml -- --check
   cargo check --manifest-path cloudflare/Cargo.toml
   ```
3. **Cloudflare Worker Wasm release build**:
   ```bash
   cd cloudflare && worker-build --release
   ```
4. **Vale Google developer style prose linting**:
   every Markdown file must follow the Google developer documentation style guide (sentence-case headings, active voice, direct instructions, second-person pronouns, and no Latin abbreviations):
   ```bash
   vale sync
   vale README.md AGENTS.md CLAUDE.md GEMINI.md docs/*.md
   ```
   When creating a new Markdown file, register it in `.vale.ini` under `BasedOnStyles = Google`, add it to `mkdocs.yml` if it belongs to the documentation site, and allowlist its path in `.gitignore` if required.
5. **MkDocs strict site build**:
   ```bash
   python3 -m venv /tmp/docs-venv
   /tmp/docs-venv/bin/pip install --quiet -r docs/requirements.txt
   /tmp/docs-venv/bin/mkdocs build --strict --site-dir /tmp/docs-site
   ```

## Git workflow and pull request review gates

A human task prompt authorizes agents to work on an isolated feature branch, commit task changes, push the branch, open a pull request against `main`, resolve review comments, and squash-merge once all review and continuous integration (CI) gates pass. It does not authorize force-pushing, modifying branch protection rules, or mutating live cloud resources.

1. **Branching and commit signing**:
   - Work on a dedicated feature branch or isolated Git worktree based on the latest `origin/main`. Never commit or push directly to `main`.
   - Stage files by explicit path (`git add <path>`) so temporary scratch files and local build artifacts are never committed.
   - Sign commits with SSH or GPG when configured in the operator environment (`commit.gpgsign=true`).
2. **Local and pull request review loop**:
   - Run the complete Rust test suite and Vale prose checks before opening a pull request.
   - Obtain an independent local review of the full diff against `origin/main` using Google Antigravity (via an independent review subagent) or OpenAI Codex (`codex exec --model gpt-6.1-sol -c model_reasoning_effort=high --sandbox read-only --ephemeral review --base origin/main`).
   - Open a pull request against `main` and obtain an independent review from Antigravity or trigger `@codex review`.
   - Fix every valid finding, rerun tests and Vale, push the commit, reply with verification evidence, and resolve the review thread. Repeat until the reviewer reports zero remaining issues on the current head commit and all GitHub checks pass.
3. **Pinned squash-merge**:
   - Squash-merge the pull request while pinning the reviewed head commit SHA so no unreviewed commit can race the merge gate:
     ```bash
     gh pr merge --squash --delete-branch --match-head-commit "$REVIEWED_HEAD_SHA"
     ```

## Multi-harness model routing

Keep the human-selected main-session model unchanged in all harnesses. When delegating bounded work to subagents:

- **Google Antigravity Desktop and IDE**: follow the full routing module specification in [GEMINI.md](GEMINI.md) and `.agents/rules/routing.md` (`Gemini 4 Argon` at `High` effort for heavy complexity and verification, `Gemini 3.8 Flash` at `High` or `Medium` effort for cross-module or bounded coding, and `Gemini 3.5 Flash-Lite` at `High` effort for fast read-only discovery).
- **ChatGPT and OpenAI Codex Desktop and command-line tool**: route delegated subagents and local review processes according to the task complexity table below:

| Delegated task | Codex model | Reasoning effort | When to use |
| --- | --- | --- | --- |
| Fast code identification, file/symbol inventory, grepping scenarios, and mechanical edits | `gpt-6-luna` | `high` | High-volume read-only search, symbol lookups, and trivial mechanical changes |
| Bounded implementation, unit test additions, or focused review with explicit contracts | `gpt-6.1-sol` | `medium` | Scoped feature work, unit tests, and Vale documentation updates |
| Cross-module debugging, `KV` lease/concurrency reasoning, or PR verification review | `gpt-6.1-sol` | `high` | Multi-file changes across `lib.rs`, `state.rs`, and `reconciler.rs`, or independent diff reviews |
| Heavy complexity tasks, ambiguous architecture, security boundaries, and state/release safety | `gpt-6-astra` | `high` | Architectural design, authentication cryptography, `KV` consistency invariants, and final judge stages |

## Claude model routing guide

This section defines the subagent routing guide specific to **Claude Desktop** and **Claude Code** (also referenced directly by [CLAUDE.md](CLAUDE.md)).

### Claude routing tiers

Applies to subagent spawns through the `Agent` tool `model` and `effort` parameters, Workflow `opts.model`, and command-line child invocations (`--model` and `--effort`). Keep the human-selected session model unchanged for main-loop orchestration, and route delegated subagents by failure impact and interacting state complexity:

- **fable / Opus 5.5 (`opus-5.5`, `high` effort)**: judgment-heavy tasks where errors have high blast radius:
  - Architecture and decomposition decisions across the Worker router, `KV` state engine, and cron reconciler.
  - State identity, deduplication, and version-ordering contracts (`state_version`, canonical `SHA-256` `state_fingerprint`, and lease expiration boundaries).
  - Fallback-tier, liveness, and terminal-state reconciliation logic.
  - Timing-flake and concurrency race diagnosis across Wasm `thread_local!` caches and `KV` storage.
  - Security triage (`HMAC-SHA256` session cookies, OAuth 2.0 flows, path normalization, real vulnerability versus false positive).
  - Workflow `verify`, `judge`, and `synthesize` stages.
- **opus / Opus 5.5 (`opus-5.5`) and Sonnet 5.5 (`sonnet-5.5`, `high` effort)**: default for substantive code changes:
  - Implementing features and bug fixes with a decided design.
  - Cross-module debugging, lock or lease reasoning, and pattern-following multi-file refactors.
  - Code review finder passes and nontrivial unit or adversarial tests in `cloudflare/tests/`.
- **sonnet / Sonnet 5.5 (`sonnet-5.5`, `medium` effort)**: mechanical, low-ambiguity, high-volume tasks:
  - Codebase exploration and search fan-out.
  - Mechanical migrations (symbol renames, applying an established pattern across files).
  - Summarizing files, build logs, or pull request review comments.
  - Test scaffolding and documentation updates from an explicit specification.
- **haiku / Haiku 5.5 (`haiku-5.5`, `high` effort)**: bulk trivial fan-out:
  - Per-file classification, grep-and-report lookups, symbol inventory, and formatting or configuration checks.

| Task category | Tier alias | Claude model | Model ID | Effort |
| --- | --- | --- | --- | --- |
| Per-file classify/grep-and-report, symbol lookup, and formatting checks | `haiku` | Haiku 5.5 | `haiku-5.5` | `high` |
| Exploration fan-out, mechanical migrations, log/PR summaries, and test scaffolding | `sonnet` | Sonnet 5.5 | `sonnet-5.5` | `medium` |
| Cross-module debugging, `KV` lease concurrency reasoning, and multi-file refactors | `sonnet` / `opus` | Sonnet 5.5 | `sonnet-5.5` | `high` |
| Heavy complexity architecture, security/auth boundaries, `KV` invariants, and verify/judge stages | `fable` / `opus` | Opus 5.5 | `opus-5.5` | `high` |

### Claude workflow stages and delegation best practices

1. **Workflow stage routing**:
   - Route `finders` and `sweepers` to `sonnet` (`sonnet-5.5` at `medium` effort, or `sonnet-5.5` at `high` / `opus-5.5` when the code involves subtle concurrency or auth invariants).
   - Route `verify`, `judge`, and `synthesize` stages to `fable` (`opus-5.5` at `high` effort).
   - Never run a stage with more than 5 concurrent agents on `fable` (`opus-5.5`) unless it is a `verify` or `judge` stage. Log (`log()`) non-default routing choices.
2. **Prefer agents as the routing mechanism**:
   - Because the session model cannot switch mid-run, subagent delegation is the only way to apply per-task model routing during autonomous or bypass-permissions runs.
   - Whenever a chunk of work is self-contained enough to hand off (exploration, mechanical migrations, review finder passes, per-file sweeps, or verification), spawn a subagent with an explicit `model` and `effort` from the preceding tiers rather than executing it inline.
   - Keep inline only work that depends on accumulated conversation context or edits that are too small to justify a handoff. Keep orchestration, judgment calls, and final synthesis in the main loop on the session model.
3. **Self-contained prompts and writer isolation**:
   - Pass every subagent a complete, self-contained brief stating the exact goal, relevant file paths, owned files, `KV` and security invariants, validation commands (`cargo test --manifest-path cloudflare/Cargo.toml` and `vale`), and required completion evidence.
   - Parallelize read-only subagents freely. Give each writing subagent exclusive file ownership or an isolated Git worktree so concurrent edits never collide.
4. **Verify resolved model and effort**:
   - Check that `CLAUDE_CODE_EFFORT_LEVEL` or global subagent model overrides do not silently downgrade the requested route. If the session model is mismatched to the task tier, state the mismatch and suggest `/model`. Never silently claim a route ran if an override blocked it.
