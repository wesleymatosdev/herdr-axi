# VERIFICATION-DISPATCH-20260905.md

## Scope

Close the dispatch-verification gap flagged in `REFACTOR-REPORT-20260905.md`
("Gaps / boundaries not exercised"): the extracted `axi_core::dispatch()`
orchestration had no standalone mock-`Backend` unit coverage, and no
full successful (idle -> working -> settled) dispatch had been exercised
live.

## What was added

`crates/axi_core/tests/dispatch_mock.rs` — a deterministic, in-memory
`FakeBackend` implementing `axi_core::Backend` (no process spawning, no
herdr, no sleeps tied to real backend state). It records every call
(`agents`/`send_text`/`send_key`/`read_pane`/`wait`) so tests assert on
the exact sequence `dispatch()` performs, and lets `send_key` mutate the
fake's reported status to simulate real state transitions.

6 new tests, all deterministic (no live process, no network, no paid
inference):

1. `dispatch_happy_path_idle_to_working_to_settled` — full success path:
   one `send_text`, exactly one Enter needed (idle->working simulated
   after 1st Enter), one settle `wait()`, result `Ok("idle")`.
2. `dispatch_unknown_agent_returns_error` — `Err(UnknownAgent)`, zero
   pane I/O performed (fails at the first `agents()` lookup, before any
   text/key is sent).
3. `dispatch_blocked_agent_self_heals_via_escape_then_proceeds` — blocked
   agent, `Escape` heals it to idle, normal dispatch then runs to
   completion.
4. `dispatch_blocked_agent_stays_blocked_returns_error` — blocked agent
   that does NOT heal on Escape returns `Err(Blocked)`, and dispatch
   never types text into a still-blocked pane.
5. `dispatch_clears_codex_onboarding_before_dispatching` — two scripted
   onboarding screens (directory-trust, then hooks-review) are cleared
   with choices `1` then `3`, each Enter-confirmed, before the normal
   type/settle/enter/wait sequence runs.
6. `dispatch_stalled_when_agent_never_leaves_idle` — agent that never
   reports `working`: all 3 Enter retries are attempted, `wait()` is
   never reached, result `Err(Stalled)`. This test genuinely burns the
   orchestration's real 5s-per-retry `wait_for_working` budget (~15s
   wall time) because `axi_core::dispatch` has no injectable clock —
   documented as an explicit, non-flaky cost, not a bug.

This is **mock coverage** — it proves the orchestration's control flow
(call order, branch conditions, error propagation) against a scripted
`Backend`, not against a real herdr process. It is not claimed as live
evidence.

## Real (live) dispatch attempt — blocked, documented honestly

Investigated whether a single isolated idle->working->settled dispatch
could be run against an **owned scratch pane/process** (per the task's
constraint: no prompting existing user panes, no paid inference, no
heavy model loading).

Checked `herdr`'s actual pane-creation primitives:

```
$ herdr agent start --help
Start a supported interactive agent in an existing pane
...
      --kind <KIND>
          Supported agent kind and canonical executable
          [possible values: pi, claude, codex, gemini, cursor, devin, agy,
           cline, omp, mastracode, opencode, copilot, kimi, kiro, droid,
           amp, grok, hermes, kilo, qodercli, qwen, maki]
```

**Finding**: every pane kind `herdr` can drive through `axi_core::dispatch`
(and thus the only thing `HerdrBackend::agents()` will ever report as a
dispatchable target) is a real interactive AI coding-agent CLI (claude,
codex, gemini, cursor, hermes, etc.). There is no lightweight/no-op/echo
agent kind. Starting *any* fresh owned pane and running a real dispatch
through it necessarily means invoking one of those live agent CLIs, which
means either paid API inference or loading a local model — both explicitly
out of scope for this task.

The two options herdr exposes are therefore both blocked:
- Prompt an existing idle pane (`spike-vec`, `luna`) — explicitly
  forbidden ("no prompting existing user panes").
- Spin up a new owned pane and `herdr agent start --kind <x>` an agent
  into it — requires paid inference / heavy model loading to get that
  pane to a real `idle` state that could then transition to `working`,
  also explicitly forbidden.

**Conclusion**: no safe real-dispatch harness is feasible under the
stated constraints. This matches (and closes out, as an explicit
decision rather than an open question) the gap noted in
`REFACTOR-REPORT-20260905.md`'s "Gaps" section. The mock-`Backend`
coverage above is the complete, honest substitute: it exercises the
exact same `dispatch<B: Backend>()` code path exercised by
`HerdrBackend` in production, generically, deterministically, and
repeatably — which is what the trait extraction was *for*.

## Gates — actually run

```
$ cargo build --workspace
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.06s

$ cargo test --workspace
axi_core::tests::core          -> 3 passed, 0 failed
axi_core::tests::dispatch_mock -> 6 passed, 0 failed   (~16.6s, see note above)
herdr::tests::fixtures         -> 5 passed, 0 failed
TOTAL: 14 passed, 0 failed   (8 original + 6 new mock-dispatch tests)

$ cargo clippy --all-targets -- -D warnings
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.05s   (clean, 0 warnings)

$ cargo fmt --all -- --check
(no output — clean)
```

No regression: the pre-existing 8 tests, clippy, and fmt all remain
exactly as they were in `REFACTOR-REPORT-20260905.md`. No `axi_core`,
`herdr`, or `cli` source changed — only a new test file was added.

## Files changed

- Added: `crates/axi_core/tests/dispatch_mock.rs` (new file, 6 tests)
- Added: `VERIFICATION-DISPATCH-20260905.md` (this report)

No other files touched. `recovery/opencode-wip-20260905` branch and
`recovery-evidence/` untouched. Nothing pushed.

## Commit

See `git log -1` on `master` at the time this report was written for the
exact SHA (recorded at commit time below).

## Remaining gaps

- Live idle->working->settled dispatch through a real herdr-driven AI
  agent pane remains unexercised, for the structural reason above (every
  herdr-drivable pane kind is a real paid/heavy-model agent CLI). This is
  not a code defect; the orchestration is unit-for-unit unchanged from
  the pre-refactor monolith (per `REFACTOR-REPORT-20260905.md`) and is
  now additionally covered end-to-end by deterministic mock tests.
- If a future task explicitly authorizes paid inference or a lightweight
  local model for exactly this purpose, or if a no-op/test `Backend`
  fixture agent kind is ever added to `herdr` itself, that would unblock
  the live half of this gap.
