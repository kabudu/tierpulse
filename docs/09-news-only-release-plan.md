# News-Only Sentiment Release

- [x] Add opt-in news-only mode without changing default legacy behaviour.
- [x] Preserve usable batch results and distinguish missing data from neutral sentiment.
- [x] Isolate cached results by mode.
- [x] Run full Rust tests (49 passed, including HTTP partial-result coverage).
- [x] Run format, lint and release consistency checks (cargo fmt, Clippy with warnings denied, v1.4.0 verification).
- [x] Resolve four CI dependency vulnerabilities; cargo audit now passes with only the existing transitive paste maintenance warning. Rerun all 49 tests after lockfile updates.
- [x] Review and merge v1.4.0 through PR #1; release checks passed but image publication exposed a Docker toolchain mismatch.
- [x] Pin the Docker toolchain before cross-target installation and prepare corrective v1.4.1 without moving the existing tag.
- [x] Verify v1.4.1 release checks and publication through the existing pipeline (run 35898028633, PR #2, commit 59056e5).
- [x] Verify versioned AMD64/ARM64 image publication before ZephyrApex pins the dependency. Published ARM64 image starts healthy and returns HTTP 200 with unavailable results for a two-symbol batch, with provider budget zero and no LLM credentials. Isolated smoke-test container removed.

Release: https://github.com/kabudu/tierpulse/releases/tag/v1.4.1

Image index: `sha256:44b82b24cc546fbfa6749553fa3449ba5a46680013275b3e92b4a1d4c4433618`.
