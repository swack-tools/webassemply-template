# Pure-Rust Cloudflare Workers + Cloudflare KV state template task runner.
# Provides test, Wasm build, formatting, and Vale Google style verification recipes.

export PATH := env_var('HOME') + "/.cargo/bin:" + env_var('PATH')

# Default recipe listing available commands.
default:
    @just --list

# Run all unit, concurrency, lease-locking, and KV quota conservation tests.
test:
    cargo test --manifest-path cloudflare/Cargo.toml

# Build Cloudflare Worker Wasm bundle and JavaScript shim in release mode.
build:
    cd cloudflare && worker-build --release

# Fast compiler and Vale prose check.
check: check-crates check-vale

# Run cargo check on the Cloudflare Worker crate.
check-crates:
    cargo check --manifest-path cloudflare/Cargo.toml

# Run Vale prose linter across all tracked Markdown files.
check-vale:
    vale README.md AGENTS.md CLAUDE.md GEMINI.md .agents/rules/routing.md docs/*.md

# Synchronize Vale packages and verify documentation prose.
vale:
    vale sync
    vale README.md AGENTS.md CLAUDE.md GEMINI.md .agents/rules/routing.md docs/*.md

# Format Rust source code.
fmt:
    cargo fmt --manifest-path cloudflare/Cargo.toml

# Check Rust source formatting without modifying files.
fmt-check:
    cargo fmt --manifest-path cloudflare/Cargo.toml -- --check

# Clean build artifacts.
clean:
    cargo clean --manifest-path cloudflare/Cargo.toml
    rm -rf cloudflare/build site
