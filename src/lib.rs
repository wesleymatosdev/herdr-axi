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
/// Accepts both shapes: `{"error":"agent_blocked"}` and
/// `{"error":{"code":"agent_not_found","message":"..."}}`.
pub fn parse_error(stdout: &str) -> HerdrError {
    let trimmed = stdout.trim();
    let v: serde_json::Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(_) => return HerdrError::Other(trimmed.to_string()),
    };
    let err = match v.get("error") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(obj) => obj
            .get("code")
            .and_then(|c| c.as_str())
            .unwrap_or_default()
            .to_string(),
        None => String::new(),
    };
    match err.as_str() {
        "agent_blocked" => HerdrError::Blocked,
        "agent_prompt_stalled" => HerdrError::Stalled,
        "agent_not_found" | "unknown_agent" => HerdrError::UnknownAgent,
        "timeout" => HerdrError::Timeout,
        _ => HerdrError::Other(trimmed.to_string()),
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

/// Return the menu choice for a one-time Codex onboarding prompt, if present.
/// Directory trust is accepted, while hooks are deliberately left untrusted.
pub fn codex_onboarding_choice(pane_content: &str) -> Option<&'static str> {
    let content = pane_content.to_ascii_lowercase();
    if content.contains("trust this directory")
        || content.contains("trust the contents of this directory")
    {
        Some("1")
    } else if content.contains("continue without trusting hooks") {
        Some("3")
    } else {
        None
    }
}

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
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        return Err(parse_error(&format!("{stdout}{stderr}")));
    }
    Ok(parse_agent_list(&stdout))
}

/// Submit a task to an agent using firstmate's verified-submit dance
/// (bin/backends/herdr.sh: fm_backend_herdr_send_text_submit):
/// type once via `pane send-text` (never retyped), then retry `pane
/// send-keys enter` (Enter only) until the agent's native state leaves idle
/// (idle -> working transition = the TUI accepted the submission), then wait
/// for the settled state via `agent wait`. A blocked agent is refused
/// up-front, mirroring herdr's own agent_blocked pre-check.
pub fn dispatch(name: &str, task: &str, timeout_ms: u64) -> Result<String, HerdrError> {
    let agents = live_agents()?;
    let agent = agents
        .iter()
        .find(|a| a.name == name)
        .ok_or(HerdrError::UnknownAgent)?;
    let pane = agent.pane_id.clone();

    if agent.agent == "codex" {
        clear_codex_onboarding(&pane)?;
    }

    // Onboarding changes the native state reported by herdr.
    let agents = live_agents()?;
    let agent = agents
        .iter()
        .find(|a| a.name == name)
        .ok_or(HerdrError::UnknownAgent)?;

    // blocked agents self-heal: one Escape dismisses the dialog (firstmate's
    // composer-clear move); only a still-blocked agent surfaces as an error.
    if agent.agent_status == "blocked" {
        run_herdr(&["pane", "send-keys", &pane, "escape"])?;
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let still_blocked = live_agents()?.iter().any(|a| {
            a.name == name && (a.agent_status == "blocked" || a.agent_status == "unknown")
        });
        if still_blocked {
            return Err(HerdrError::Blocked);
        }
    }

    // 1. type the text once, unsubmitted
    run_herdr(&["pane", "send-text", &pane, task])?;

    // 2. settle so completion popups / TUI redraws cannot swallow the Enter
    std::thread::sleep(std::time::Duration::from_millis(400));

    // 3. retry Enter only, watching for the idle -> working transition
    let mut accepted = agent.agent_status == "working";
    for _ in 0..3 {
        run_herdr(&["pane", "send-keys", &pane, "enter"])?;
        if wait_for_working(name, 5_000)? {
            accepted = true;
            break;
        }
    }
    if !accepted {
        return Err(HerdrError::Stalled);
    }

    // 4. wait for the settled state (herdr default: idle, done, or blocked)
    wait(name, None, Some(timeout_ms))
}

/// Clear Codex's one-time onboarding chain before normal dispatch. The cap
/// prevents an unexpected pane from causing an unbounded interaction loop.
fn clear_codex_onboarding(pane: &str) -> Result<(), HerdrError> {
    for _ in 0..5 {
        let content = run_herdr(&[
            "pane",
            "read",
            pane,
            "--source",
            "detection",
            "--format",
            "text",
        ])?;
        let Some(choice) = codex_onboarding_choice(&content) else {
            break;
        };
        run_herdr(&["pane", "send-keys", pane, choice, "enter"])?;
        std::thread::sleep(std::time::Duration::from_millis(400));
    }
    Ok(())
}

/// Run a herdr subcommand, mapping failure through parse_error.
fn run_herdr(args: &[&str]) -> Result<String, HerdrError> {
    let out = Command::new(herdr_path())
        .args(args)
        .output()
        .map_err(|e| HerdrError::Other(format!("failed to spawn herdr: {e}")))?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        return Err(parse_error(&format!("{stdout}{stderr}")));
    }
    Ok(stdout)
}

/// Poll agent state until `name` reports working (submission accepted).
/// Samples ~every 300ms across the budget; a transition landing partway
/// through is still caught (firstmate's wait_for_working pattern).
fn wait_for_working(name: &str, budget_ms: u64) -> Result<bool, HerdrError> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(budget_ms);
    while std::time::Instant::now() < deadline {
        if let Ok(agents) = live_agents() {
            let working = agents
                .iter()
                .any(|a| a.name == name && a.agent_status == "working");
            if working {
                return Ok(true);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    Ok(false)
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
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        return Err(parse_error(&format!("{stdout}{stderr}")));
    }
    Ok(stdout.trim().to_string())
}
