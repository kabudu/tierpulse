# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

### Changed

### Fixed

## [1.3.1] - 2026-08-21

### Fixed

- Changed the default DeepSeek Tier-3 model from `deepseek-v4-pro` to
  `deepseek-v4-flash` while preserving the `TP_DEEPSEEK_MODEL` override.

## [1.3.0] - 2026-05-30

### Added

- Added configurable Tier-3 LLM execution order via `TP_LLM_PROVIDER_ORDER`.
- Added configurable model names for Grok, DeepSeek, and OpenAI via `TP_GROK_MODEL`, `TP_DEEPSEEK_MODEL`, and `TP_OPENAI_MODEL`.
- Added OpenAI as a Tier-3 LLM provider using `TP_OPENAI_KEY` and `TP_OPENAI_MODEL`.
- Added OpenAI egress allowlist support for `api.openai.com`.
- Added `.env.example` and Docker Compose `.env` loading for local development secrets.
- Added provider JSON-mode requests and parser support for the LLM `{ "results": [...] }` response contract.
- Added `rust-toolchain.toml` pinned to Rust 1.96.0 with `rustfmt` and `clippy`.
- Added configurable ONNX model export via Docker build arg `MODEL_ID` / export script `--model-id`.
- Added exported `model_labels.json` so runtime label interpretation follows the selected model's `id2label` mapping.
- Added release consistency checks tying `v*` Git tags to `Cargo.toml` version and `CHANGELOG.md` release sections before Docker publishing.
- Added automatic GitHub Release creation from the matching `CHANGELOG.md` release section after tagged Docker publishes succeed.

### Changed

- Migrated the crate to Rust 2024 edition with `rust-version = "1.96"`.
- Updated root Rust dependencies to current compatible releases, including Axum 0.8, Redis 1.2, Reqwest 0.13, JsonWebToken 10, Governor 0.10, Tokenizers 0.23, and Axum Test 20.
- Updated Docker and CI toolchains to Rust 1.96.0.
- Tier-3 LLM fallback now advances through the configured provider order after request errors, non-2xx responses, or invalid response payloads.
- Empty optional credential environment variables are treated as unset.
- Updated default Tier-3 models to current provider slugs: `grok-4.3`, `deepseek-v4-pro`, and `gpt-5.4-nano`.
- Readiness output now includes OpenAI provider status and the active LLM execution order.
- Updated DeepSeek to the current `https://api.deepseek.com/chat/completions` endpoint and removed OpenAI-specific payload fields from shared LLM requests.
