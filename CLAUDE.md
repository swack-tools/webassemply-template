@read AGENTS.md
@AGENTS.md

## Claude model routing policy

This policy applies to Claude subagent launches: the `Agent` tool `model` and `effort` parameters, Workflow `opts.model`, and command-line child processes (`--model` and `--effort`). Keep the human-selected session model unchanged for the main orchestration loop. When spawning subagents, deviate only according to the tiers below. If the active session model does not match the task tier, state the mismatch and suggest `/model`. Never proceed silently when a route or effort override fails to apply.

- **fable / Opus 5.5 (`opus-5.5`, `high` effort)**: judgment-heavy tasks with a high cost of being wrong:
  - Architecture and module decomposition decisions across the Cloudflare Worker and `KV` state engine.
  - State identity, deduplication, and ordering contracts (`state_version` monotonicity, `SHA-256` `state_fingerprint` canonicalization, and distributed `KV` lease expiration).
  - Fallback-tier, liveness, and terminal-state reconciliation logic in `cloudflare/src/reconciler.rs`.
  - Timing-flake and concurrency race debugging across Wasm isolate caches (`thread_local!`) and `KV` writes.
  - Security triage and authentication boundaries (`HMAC-SHA256` session cookies, `SESSION_NOT_BEFORE` revocation, and path normalization).
  - Workflow verification, judgment, and synthesis stages.
- **opus / Opus 5.5 (`opus-5.5`) and Sonnet 5.5 (`sonnet-5.5`, `high` effort)**: default for substantive code:
  - Implementing Worker endpoints, `KV` state transitions, and bug fixes from a decided design.
  - Cross-module debugging spanning `lib.rs`, `state.rs`, `auth.rs`, and `reconciler.rs`.
  - Code review finder passes and pattern-following multi-file refactors.
  - Nontrivial unit, concurrency, and adversarial test suites in `cloudflare/tests/`.
- **sonnet / Sonnet 5.5 (`sonnet-5.5`, `medium` effort)**: mechanical, low-ambiguity, high-volume work:
  - Codebase exploration and search fan-out across crates and documentation.
  - Mechanical migrations such as symbol renames or applying a decided pattern across files.
  - Summarizing files, build logs, or pull request review comments.
  - Test scaffolding and Vale-compliant documentation updates from an explicit specification.
- **haiku / Haiku 5.5 (`haiku-5.5`, `high` effort)**: bulk trivial fan-out:
  - Per-file classification, grep-and-report symbol lookups, and read-only configuration or formatting checks.

| Tier alias | Claude model | Model ID | Effort | Primary workload |
| --- | --- | --- | --- | --- |
| `haiku` | Haiku 5.5 | `haiku-5.5` | `high` | Per-file grep-and-report, symbol inventory, and formatting checks |
| `sonnet` (mechanical) | Sonnet 5.5 | `sonnet-5.5` | `medium` | Exploration fan-out, mechanical refactors, log summaries, and test scaffolding |
| `sonnet` (substantive) | Sonnet 5.5 | `sonnet-5.5` | `high` | Cross-module debugging, lease reasoning, and multi-file implementation |
| `opus` / `fable` | Opus 5.5 | `opus-5.5` | `high` | Architecture, `KV` consistency contracts, security boundaries, and verify/judge stages |

### Workflow stages and delegation rules

- **Workflow stage mapping**: route `finders` and `sweepers` to `sonnet` (`sonnet-5.5` at `medium` effort, or `high` / `opus-5.5` when the target code involves subtle state invariants). Route `verify`, `judge`, and `synthesize` stages to `fable` (`opus-5.5` at `high` effort). Never run a stage with more than 5 concurrent agents on `fable` (`opus-5.5`) unless it is a `verify` or `judge` stage. Record non-default routing decisions with `log()`.
- **Prefer agents as the routing mechanism**: the session model cannot switch mid-run. In autonomous or bypass-permissions runs, subagent delegation is the mechanism that enforces this routing policy. When a unit of work is self-contained enough to hand off (exploration, mechanical migrations, review finder passes, per-file sweeps, or independent verification), spawn a subagent with an explicit `model` and `effort` from the preceding tiers rather than running it inline.
- **Keep context-bound orchestration inline**: retain in the main session only work that depends on accumulated conversation context or edits that are too small to justify a handoff. Orchestration, architectural judgment calls, and final synthesis remain in the main loop on the session model.
- **Provide self-contained subagent briefs**: every delegated task must carry its complete context in the prompt, including the exact goal, target file paths, owned files, `KV` quota and security invariants to preserve, verification commands (`cargo test --manifest-path cloudflare/Cargo.toml` and `vale`), and required completion evidence. Give independent review agents the raw diff and acceptance contract without the implementer's conclusions.
- **Isolate concurrent writers**: parallelize read-only exploration freely. Assign each writing subagent an exclusive file set or an isolated Git worktree so concurrent edits never collide.
- **Verify resolved model and effort**: environment variables such as `CLAUDE_CODE_EFFORT_LEVEL` or forced subagent model overrides can supersede per-launch parameters. If the active harness lacks per-invocation effort support, launch an authorized command-line child with explicit `--model` and `--effort` flags or report the mismatch rather than claiming the requested route ran.
