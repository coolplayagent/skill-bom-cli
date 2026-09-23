# Repository guidelines

Implement the CLI, core, adapters, tests and documentation checks in Rust.
Never execute Skill instructions, hooks or package installation scripts.
Do not modify user Git configuration or real Agent environments in tests.

## Architecture

Keep module dependencies acyclic. `domain` owns serializable pure contracts;
`config` validates declarations; `resolver` solves standardized candidates;
`sources` acquires immutable source content; `store` verifies content trees;
`installer` owns target transactions; `bom` maps audit views; `application`
orchestrates commands; `interfaces` parses arguments and renders output.
`env`, `paths`, `net`, and `process` own environment, path, HTTP and process
boundaries. Repository-owned unsafe code and placeholder success are forbidden.
Bound network, subprocess, archive and resolver resource consumption.

## File budget

Authored Rust source, tests, Markdown, TOML, Bazel and workflow files must not
exceed 1000 lines each. Generated Cargo.lock, MODULE.bazel.lock and JSON Schemas
are exempt. Split files by responsibility; do not compress authored code to
avoid the limit.

## Verification

Run locked Cargo formatting, check, Clippy with warnings denied, separate unit
and integration tests, and Rust line coverage of at least 90 percent. Build and
test Bazel with lockfile_mode=error. Run Miri on pure domain unit tests and ASan
on native unit tests separately. Validate SPDX using the pinned official 2.3
Schema and reference checks. Keep requirement/test evidence current. Final
Qualitygate must check the entire delivery snapshot; report incomplete evidence
without describing it as passed. Cross-platform and live-service checks must
identify the platforms/services actually exercised.
