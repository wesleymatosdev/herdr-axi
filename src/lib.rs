//! herdr-axi: AXI-discipline wrapper around the `herdr` multiplexer CLI.
//!
//! Pure core (parsing, formatting, error mapping) plus a thin spawn layer
//! that shells out to `herdr` and feeds its JSON through the core.

use std::collections::BTreeMap;
use std::process::Command;

/// A live agent as reported by `herdr agent list`.
#[derive(Debug, Clone, PartialEq)]
pub struct Agent {
    pub name: String,
    pub agent: String,
    pub pane_id: String,
    pub agent_status: String,
}

/// Error shapes herdr reports back over its CLI, mapped to AXI exit codes.
#[derive(Debug, Clone, PartialEq)]
pub enum HerdrError {
    /// agent at an approval dialog; human must answer in the pane.
    Blocked,
    /// no lifecycle change observed within 5s of submission.
    Stalled,
    /// target agent does not exist.
    UnknownAgent,
    /// the wait timed out.
    Timeout,
    /// anything else; carries the raw herdr output.
    Other(String),
}

impl HerdrError {
    /// AXI exit code: 3 = blocked, 4 = stalled, 2 = unknown agent, 5 = timeout.
    pub fn exit_code(&self) -> i32 {
        match self {
            HerdrError::Blocked => 3,
            HerdrError::Stalled => 4,
            HerdrError::UnknownAgent => 2,
            HerdrError::Timeout => 5,
            HerdrError::Other(_) => 1,
        }
    }

    /// (cause, action) pair for structured error output.
    pub fn cause_action(&self) -> (String, String) {
        match self {
            HerdrError::Blocked => (
                "agent is sitting at an approval dialog".into(),
                "inspect the pane; a human must answer it".into(),
            ),
            HerdrError::Stalled => (
                "no lifecycle change within 5s of submission".into(),
                "check the agent exists and its pane is alive".into(),
            ),
            HerdrError::UnknownAgent => (
                "no live agent with that name".into(),
                "run `herdr-axi agents` to see live agents".into(),
            ),
            HerdrError::Timeout => (
                "wait exceeded the timeout".into(),
                "raise --timeout or check the agent's state with `herdr-axi agents`".into(),
            ),
            HerdrError::Other(raw) => (
                raw.clone(),
                "re-run with the raw herdr CLI to investigate".into(),
            ),
        }
    }
}

/// Parse the JSON emitted by `herdr agent list`.
pub fn parse_agent_list(stdout: &str) -> Vec<Agent> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(stdout) else {
        return Vec::new();
    };
    let Some(list) = v
        .get("result")
        .and_then(|r| r.get("agents"))
        .and_then(|a| a.as_array())
    else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|a| {
            Some(Agent {
                name: a.get("name")?.as_str()?.to_string(),
                agent: a.get("agent")?.as_str().unwrap_or("?").to_string(),
                pane_id: a.get("pane_id")?.as_str()?.to_string(),
                agent_status: a
                    .get("agent_status")?
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string(),
            })
        })
        .collect()
}

/// Parse an error out of herdr's JSON failure response (or raw text fallback).
pub fn parse_error(stdout: &str) -> HerdrError {
    let v: serde_json::Value = match serde_json::from_str(stdout) {
        Ok(v) => v,
        Err(_) => return HerdrError::Other(stdout.trim().to_string()),
    };
    let err = v.get("error").and_then(|e| e.as_str()).unwrap_or_default();
    match err {
        "agent_blocked" => HerdrError::Blocked,
        "agent_prompt_stalled" => HerdrError::Stalled,
        "agent_not_found" | "unknown_agent" => HerdrError::UnknownAgent,
        "timeout" => HerdrError::Timeout,
        _ => HerdrError::Other(stdout.trim().to_string()),
    }
}

/// State -> count map for JSON fleet output.
pub fn fleet_counts(agents: &[Agent]) -> BTreeMap<String, usize> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for a in agents {
        *counts.entry(a.agent_status.clone()).or_default() += 1;
    }
    counts
}

/// Render the agent table (human form). Empty input gives the definitive empty state.
pub fn format_agents_table(agents: &[Agent]) -> String {
    if agents.is_empty() {
        return "no live agents".to_string();
    }
    let mut out = String::from("NAME          KIND      PANE      STATE\n");
    for a in agents {
        out.push_str(&format!(
            "{:<14}{:<10}{:<10}{}\n",
            a.name, a.agent, a.pane_id, a.agent_status
        ));
    }
    out
}

/// Pre-computed fleet aggregate: counts by state plus names of blocked agents,
/// e.g. `2 working, 1 idle, 1 blocked (reviewer)`.
pub fn format_fleet(agents: &[Agent]) -> String {
    if agents.is_empty() {
        return "no live agents".to_string();
    }
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for a in agents {
        *counts.entry(a.agent_status.as_str()).or_default() += 1;
    }
    let mut parts: Vec<String> = Vec::new();
    let blocked: Vec<&str> = agents
        .iter()
        .filter(|a| a.agent_status == "blocked")
        .map(|a| a.name.as_str())
        .collect();
    // stable, readable order; blocked count carries agent names inline
    for state in ["working", "idle", "blocked", "done", "unknown"] {
        if let Some(n) = counts.get(state) {
            if state == "blocked" && !blocked.is_empty() {
                parts.push(format!("{n} blocked ({})", blocked.join(", ")));
            } else {
                parts.push(format!("{n} {state}"));
            }
        }
    }
    // anything nonstandard still gets counted
    for (state, n) in &counts {
        if !["working", "idle", "blocked", "done", "unknown"].contains(state) {
            parts.push(format!("{n} {state}"));
        }
    }
    parts.join(", ")
}

/// Where the herdr binary lives.
pub const HERDR_BIN: &str = "~/.local/bin/herdr";

fn herdr_path() -> String {
    if let Ok(home) = std::env::var("HOME") {
        return format!("{home}/.local/bin/herdr");
    }
    HERDR_BIN.to_string()
}

/// Run `herdr agent list` and parse it.
pub fn live_agents() -> Result<Vec<Agent>, HerdrError> {
    let out = Command::new(herdr_path())
        .args(["agent", "list"])
        .output()
        .map_err(|e| HerdrError::Other(format!("failed to spawn herdr: {e}")))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    if !out.status.success() {
        return Err(parse_error(&String::from_utf8_lossy(&out.stdout)));
    }
    Ok(parse_agent_list(&stdout))
}

/// Submit a prompt via `herdr agent prompt --wait`.
pub fn dispatch(name: &str, task: &str, timeout_ms: u64) -> Result<String, HerdrError> {
    let out = Command::new(herdr_path())
        .args([
            "agent",
            "prompt",
            name,
            task,
            "--wait",
            "--timeout",
            &timeout_ms.to_string(),
        ])
        .output()
        .map_err(|e| HerdrError::Other(format!("failed to spawn herdr: {e}")))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    if !out.status.success() {
        return Err(parse_error(&String::from_utf8_lossy(&out.stdout)));
    }
    Ok(stdout.trim().to_string())
}

/// Wait for an agent to reach a state via `herdr agent wait`.
pub fn wait(
    name: &str,
    until: Option<&str>,
    timeout_ms: Option<u64>,
) -> Result<String, HerdrError> {
    let mut args = vec!["agent", "wait", name];
    if let Some(u) = until {
        args.push("--until");
        args.push(u);
    }
    if let Some(t) = timeout_ms {
        let leaked: &'static str = Box::leak(t.to_string().into_boxed_str());
        args.push("--timeout");
        args.push(leaked);
    }
    let out = Command::new(herdr_path())
        .args(&args)
        .output()
        .map_err(|e| HerdrError::Other(format!("failed to spawn herdr: {e}")))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    if !out.status.success() {
        return Err(parse_error(&String::from_utf8_lossy(&out.stdout)));
    }
    Ok(stdout.trim().to_string())
}
