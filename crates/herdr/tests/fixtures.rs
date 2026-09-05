//! Tests against JSON fixtures captured from real `herdr agent list` output
//! (herdr 0.8.2), plus herdr-specific error-mapping coverage and the
//! parse -> format integration through axi_core's formatting helpers.

use herdr::{parse_agent_list, parse_error};

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
    assert_eq!(axi_core::format_agents_table(&[]), "no live agents");
    let agents = parse_agent_list(AGENT_LIST_FIXTURE);
    let table = axi_core::format_agents_table(&agents);
    assert!(table.contains("fixbuild"));
    assert!(table.contains("w6:p4"));
    assert!(table.contains("idle"));
}

#[test]
fn fleet_aggregate_line() {
    let agents = parse_agent_list(AGENT_LIST_FIXTURE);
    // 1 idle + 1 blocked, blocked agent named
    assert_eq!(
        axi_core::format_fleet(&agents),
        "1 idle, 1 blocked (spike-vec)"
    );
    assert_eq!(axi_core::format_fleet(&[]), "no live agents");
}

#[test]
fn error_mapping() {
    use axi_core::AxiError;
    // real herdr 0.8.2 shapes (nested code + flat string) both map
    assert_eq!(
        parse_error(
            r#"{"error":{"code":"agent_blocked","message":"agent x is blocked and requires interactive input"},"id":"cli:agent:prompt"}"#
        ),
        AxiError::Blocked
    );
    assert_eq!(
        parse_error(
            r#"{"error":{"code":"agent_blocked","message":"blocked"},"id":"cli:agent:prompt"}"#
        )
        .exit_code(),
        3
    );
    assert_eq!(
        parse_error(r#"{"error":"agent_prompt_stalled"}"#),
        AxiError::Stalled
    );
    assert_eq!(
        parse_error(r#"{"error":"agent_prompt_stalled"}"#).exit_code(),
        4
    );
    assert_eq!(
        parse_error(
            r#"{"error":{"code":"agent_not_found","message":"agent target no-such-agent not found"},"id":"cli:agent:prompt"}"#
        ),
        AxiError::UnknownAgent
    );
    assert_eq!(
        parse_error(
            r#"{"error":{"code":"agent_not_found","message":"not found"},"id":"cli:agent:prompt"}"#
        )
        .exit_code(),
        2
    );
    assert_eq!(
        parse_error("plain text failure"),
        AxiError::Other("plain text failure".into())
    );
    assert_eq!(parse_error("plain text failure").exit_code(), 1);
}
