# News-Only Sentiment Release

- [x] Add opt-in news-only mode without changing default legacy behaviour.
- [x] Preserve usable batch results and distinguish missing data from neutral sentiment.
- [x] Isolate cached results by mode.
- [x] Run full Rust tests (49 passed, including HTTP partial-result coverage).
- [x] Run format, lint and release consistency checks (cargo fmt, Clippy with warnings denied, v1.4.0 verification).
- [x] Resolve four CI dependency vulnerabilities; cargo audit now passes with only the existing transitive paste maintenance warning. Rerun all 49 tests after lockfile updates.
- [x] Review and merge v1.4.0 through PR #1; release checks passed but image publication exposed a Docker toolchain mismatch.
- [x] Pin the Docker toolchain before cross-target installation and prepare corrective v1.4.1 without moving the existing tag.
- [ ] Verify v1.4.1 release checks and publication through the existing pipeline.
- [ ] Verify versioned image publication before ZephyrApex pins the dependency.
