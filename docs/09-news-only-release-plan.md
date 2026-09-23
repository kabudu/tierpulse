# News-Only Sentiment Release

- [x] Add opt-in news-only mode without changing default legacy behaviour.
- [x] Preserve usable batch results and distinguish missing data from neutral sentiment.
- [x] Isolate cached results by mode.
- [x] Run full Rust tests (49 passed, including HTTP partial-result coverage).
- [x] Run format, lint and release consistency checks (cargo fmt, Clippy with warnings denied, v1.4.0 verification).
- [x] Resolve four CI dependency vulnerabilities; cargo audit now passes with only the existing transitive paste maintenance warning. Rerun all 49 tests after lockfile updates.
- [ ] Review, PR, merge and publish v1.4.0 through the existing release pipeline.
- [ ] Verify versioned image publication before ZephyrApex pins the dependency.
