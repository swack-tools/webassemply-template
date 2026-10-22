# Antigravity workspace routing rules

Always apply the model routing modules defined in [GEMINI.md](../../GEMINI.md) and [AGENTS.md](../../AGENTS.md):

- **Gemini 4 Argon (`gemini-4-argon`, `Model: "pro"` or `"inherit"`) at `High` effort**: use for heavy complexity tasks, architecture decisions, Cloudflare KV consolidated state and lease-locking invariants, HMAC-SHA256 authentication security, race debugging, and final verification or judge stages.
- **Gemini 3.8 Flash (`gemini-3.8-flash`, `Model: "flash"`) at `High` effort**: use for cross-module debugging, state-machine or reconciler tracing, multi-file refactors, and concurrency or quota-conservation test suites.
- **Gemini 3.8 Flash (`gemini-3.8-flash`, `Model: "flash"`) at `Medium` effort**: use for bounded feature implementation, unit test additions, mechanical migrations, and Vale-compliant documentation updates.
- **Gemini 3.5 Flash-Lite (`gemini-3.5-flash-lite`, `Model: "flash_lite"`) at `High` effort**: use for fast read-only discovery, symbol/file inventory, grep-and-report fan-out, and formatting or configuration checks.
- Isolate concurrent writing subagents with `Workspace: "share"` or `Workspace: "branch"`, and keep read-only `research` subagents on `Workspace: "inherit"`.
