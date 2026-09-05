//! Pure core coverage: onboarding-prompt classification, fleet aggregation
//! ordering, and structured error output. Backend-free — no herdr wire
//! shapes touched here.

use axi_core::{Agent, AxiError, codex_onboarding_choice, format_fleet};

const CODEX_DIRECTORY_TRUST_FIXTURE: &str = r#"
Do you trust the contents of this directory?

  1. Yes, continue
  2. No, exit
"#;

const CODEX_HOOKS_REVIEW_FIXTURE: &str = r#"
Hooks review

  1. Trust hooks and continue
  2. Review hooks
  3. Continue without trusting hooks
"#;

#[test]
fn detects_codex_onboarding_prompts() {
    assert_eq!(
        codex_onboarding_choice(CODEX_DIRECTORY_TRUST_FIXTURE),
        Some("1")
    );
    assert_eq!(
        codex_onboarding_choice(CODEX_HOOKS_REVIEW_FIXTURE),
        Some("3")
    );
    assert_eq!(codex_onboarding_choice("codex is ready for a task"), None);
}

#[test]
fn fleet_all_states_ordering() {
    let mk = |name: &str, st: &str| Agent {
        name: name.into(),
        agent: "claude".into(),
        pane_id: "p".into(),
        agent_status: st.into(),
    };
    let agents = vec![
        mk("a", "done"),
        mk("b", "working"),
        mk("c", "working"),
        mk("d", "blocked"),
        mk("e", "unknown"),
    ];
    assert_eq!(
        format_fleet(&agents),
        "2 working, 1 blocked (d), 1 done, 1 unknown"
    );
}

#[test]
fn blocked_error_structured_output() {
    let (cause, action) = AxiError::Blocked.cause_action();
    assert!(cause.contains("approval dialog"));
    assert!(action.contains("pane"));
}
