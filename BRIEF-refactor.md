# BRIEF: extract axi-core (herdr-axi refactor)

## Status: DONE (2026-09-05)

Completed after a local OpenCode worker left the repo broken (stubs only,
no `fn main`, cargo check E0601). Its dirty diff + untracked `crates/` are
preserved verbatim on branch `recovery/opencode-wip-20260905` (commit
`4ec6b80`) — see `recovery-evidence/20260905/README.md`. Recovery work
restarted from the untouched original HEAD (`e03a428`) and re-did the
extraction for real. See `REFACTOR-REPORT-20260905.md` for commits, test
output, and CLI compatibility proof.

Final shape (matches the plan below exactly):
- `crates/axi_core`: `Agent`, `AxiError` (renamed from `HerdrError` per the
  brief's own framing — "AxiError: THE error contract of the crate"),
  `Backend` trait, `format_agents_table`/`format_fleet`/`fleet_counts`,
  `codex_onboarding_choice`, and the generic `dispatch()` orchestration
  (type-once, settle, Enter-retry, blocked self-heal) written against
  `Backend`.
- `crates/herdr`: `HerdrBackend` (impl of `Backend`), `parse_agent_list`,
  `parse_error` (herdr wire-shape -> `AxiError` translation), `herdr_path()`,
  process spawning. Named `herdr` (crate id), package renders as `herdr`
  in Cargo.toml — brief said "crates/herdr" without an `_impl` suffix in
  its own module numbering (item 2), so the crate keeps that name.
- `crates/cli`: unchanged clap surface, bin name `herdr-axi`.

All 8 pre-refactor tests preserved (split axi_core::tests/core.rs [3] +
herdr::tests/fixtures.rs [5]); gate said "7 existing", actual original
count was 8 — noted as a brief inaccuracy, not a regression.


You are working in /Users/wesleymatos/projects/personal/herdr-axi
(git repo, HEAD bd16a44, Rust 2024 edition, edition-2024 toolchain required).
Work directly on the current branch. NEVER push.

## Context

herdr-axi is an AXI-discipline CLI wrapping the herdr multiplexer
(~/.local/bin/herdr, v0.8.2). Current shape: src/lib.rs holds parsing,
formatting, error mapping AND all herdr process spawning; src/main.rs is the
clap CLI. That coupling is what we are removing.

## Mission

Turn the repo into a Cargo workspace with the herdr-specific I/O isolated so
a second backend (tmux) can be added later without touching the core:

1. crates/axi-core — backend-agnostic library:
   - Agent struct
   - AxiError: THE error contract of the crate. It defines what an axi
     error IS (variants like UnknownAgent, Blocked, Stalled, Timeout,
     Other + exit_code()/cause_action()). Core knows nothing about herdr's
     JSON error shapes.
   - parse/format helpers that take already-extracted data (agent rows,
     fleet counts): format_agents_table, format_fleet, fleet_counts
   - a small `Backend` trait (e.g. `fn agents(&self) -> Result<Vec<Agent>,
     AxiError>>, fn send_text(&self, pane, text) -> Result<(), AxiError>,
     fn send_key(&self, pane, key) -> Result<(), AxiError>,
     fn wait(&self, name, until, timeout) -> Result<Waited, AxiError>`)
     with the dispatch orchestration (type-once, settle, Enter-retry
     confirmed by idle->working, then settled-state wait) written
     generically against that trait in the core. The blocked-pane
     self-heal (one Escape, re-check) also lives here.
2. crates/herdr — the herdr Backend impl: process spawning (current
     live_agents/run_herdr/wait_for_working mechanics), herdr_path(), and
     the herdr-JSON -> AxiError translation (parse_error lives HERE —
     herdr's wire error shapes are herdr's problem; it maps them into
     axi_core::AxiError at the boundary).
3. crates/cli — the existing clap binary (bin name stays `herdr-axi`),
     wired to the core through the herdr backend.
4. Keep ALL existing behavior and exit codes identical. The fixture tests in
   tests/ must keep passing (move/adjust imports into the workspace as
   needed; fixtures test pure functions and stay backend-free).
5. Add unit tests for anything you newly split out; do not delete coverage.

## Gates (all must pass before you claim done)

- cargo test --workspace  (7 existing tests + any new ones, 0 failures)
- cargo clippy --all-targets -- -D warnings  (clean)
- cargo fmt applied
- One commit per milestone with clear messages (workspace split, trait
  extraction, backend crate, CLI rewire — your judgment on the exact cuts)
- NEVER push, NEVER open PRs

## Constraints

- Rust edition 2024, stdlib + clap + serde_json only (already in lockfile).
  No async runtime.
- Do not change the CLI surface (flags, exit codes, output formats).
- If a gate fails, fix it before committing that milestone.

## Report

End by printing: final commit sha, the tail of `cargo test --workspace`,
and `cargo clippy --all-targets -- -D warnings` output.
