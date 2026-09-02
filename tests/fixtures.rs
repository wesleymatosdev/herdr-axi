//! Tests against JSON fixtures captured from real `herdr agent list` output
//! (herdr 0.8.2), plus formatting and error-mapping coverage.

use herdr_axi::*;

/// Fixture: verbatim from `~/.local/bin/herdr agent list` on 2026-09-02
/// (names/statuses kept, two agents incl. one blocked).
const AGENT_LIST_FIXTURE: &str = r#"{"id":"cli:agent:list","result":{"agents":[{"agent":"codex","agent_status":"idle","cwd":"/tmp/x","focused":false,"foreground_cwd":"/tmp/x","interactive_ready":true,"name":"fixbuild","pane_id":"w6:p4","revision":0,"state_change_seq":36,"tab_id":"w6:t1","terminal_id":"term_a","workspace_id":"w6"},{"agent":"claude","agent_status":"blocked","cwd":"/tmp/y","focused":false,"foreground_cwd":"/tmp/y","interactive_ready":true,"name":"spike-vec","pane_id":"w6:p3","revision":2,"state_change_seq":26,"tab_id":"w6:t2","terminal_id":"term_b","terminal_title":"x","terminal_title_stripped":"x","workspace_id":"w6"}]},"type":"agent_list"}"#;

#[test]
fn parses_agent_list_fixture() {
    let agents = parse_agent_list(AGENT_LIST_FIXTURE);
    assert_eq!(agents.len(), 2);
    assert_eq!(agents[0].name, "fixbuild");
    assert_eq!(agents[0].agent, "codex");
    assert_eq!(agents[0].pane_id, "w6:p4");
    assert_eq!(agents[0].agent_status, "idle");
    assert_eq!(agents[1].agent_status, "blocked");
}

#[test]
fn agent_list_empty_and_garbage() {
    assert!(
        parse_agent_list(r#"{"id":"cli:agent:list","result":{"agents":[]},"type":"agent_list"}"#)
            .is_empty()
    );
    assert!(parse_agent_list("not json at all").is_empty());
}

#[test]
fn table_and_empty_state() {
    assert_eq!(format_agents_table(&[]), "no live agents");
    let agents = parse_agent_list(AGENT_LIST_FIXTURE);
    let table = format_agents_table(&agents);
    assert!(table.contains("fixbuild"));
    assert!(table.contains("w6:p4"));
    assert!(table.contains("idle"));
}

#[test]
fn fleet_aggregate_line() {
    let agents = parse_agent_list(AGENT_LIST_FIXTURE);
    // 1 idle + 1 blocked, blocked agent named
    assert_eq!(format_fleet(&agents), "1 idle, 1 blocked (spike-vec)");
    assert_eq!(format_fleet(&[]), "no live agents");
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
fn error_mapping() {
    assert_eq!(
        parse_error(r#"{"error":"agent_blocked"}"#),
        HerdrError::Blocked
    );
    assert_eq!(parse_error(r#"{"error":"agent_blocked"}"#).exit_code(), 3);
    assert_eq!(
        parse_error(r#"{"error":"agent_prompt_stalled"}"#),
        HerdrError::Stalled
    );
    assert_eq!(
        parse_error(r#"{"error":"agent_prompt_stalled"}"#).exit_code(),
        4
    );
    assert_eq!(
        parse_error(r#"{"error":"agent_not_found"}"#),
        HerdrError::UnknownAgent
    );
    assert_eq!(parse_error(r#"{"error":"agent_not_found"}"#).exit_code(), 2);
    assert_eq!(
        parse_error("plain text failure"),
        HerdrError::Other("plain text failure".into())
    );
    assert_eq!(parse_error("plain text failure").exit_code(), 1);
}

#[test]
fn blocked_error_structured_output() {
    let (cause, action) = HerdrError::Blocked.cause_action();
    assert!(cause.contains("approval dialog"));
    assert!(action.contains("pane"));
}
