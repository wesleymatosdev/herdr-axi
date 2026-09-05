//! axi_core: backend-agnostic core of an AXI-discipline CLI.
//!
//! Owns the `Agent` model, the `AxiError` contract (what an axi error IS —
//! independent of any wire format a specific backend speaks), pure
//! formatting helpers, and the generic dispatch orchestration written
//! against the `Backend` trait. No process spawning and no knowledge of any
//! backend's JSON wire shapes live here.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// A live agent as reported by a backend's agent listing.
#[derive(Debug, Clone, PartialEq)]
pub struct Agent {
    pub name: String,
    pub agent: String,
    pub pane_id: String,
    pub agent_status: String,
}

/// The AXI error contract: what an axi error IS. Core knows nothing about
/// any backend's wire error shapes; backends translate their own errors
/// into these variants at the boundary.
#[derive(Debug, Clone, PartialEq)]
pub enum AxiError {
    /// agent at an approval dialog; human must answer in the pane.
    Blocked,
    /// no lifecycle change observed within 5s of submission.
    Stalled,
    /// target agent does not exist.
    UnknownAgent,
    /// the wait timed out.
    Timeout,
    /// anything else; carries the raw backend output.
    Other(String),
}

impl AxiError {
    /// AXI exit code: 3 = blocked, 4 = stalled, 2 = unknown agent, 5 = timeout.
    pub fn exit_code(&self) -> i32 {
        match self {
            AxiError::Blocked => 3,
            AxiError::Stalled => 4,
            AxiError::UnknownAgent => 2,
            AxiError::Timeout => 5,
            AxiError::Other(_) => 1,
        }
    }

    /// (cause, action) pair for structured error output.
    pub fn cause_action(&self) -> (String, String) {
        match self {
            AxiError::Blocked => (
                "agent is sitting at an approval dialog".into(),
                "inspect the pane; a human must answer it".into(),
            ),
            AxiError::Stalled => (
                "no lifecycle change within 5s of submission".into(),
                "check the agent exists and its pane is alive".into(),
            ),
            AxiError::UnknownAgent => (
                "no live agent with that name".into(),
                "run `herdr-axi agents` to see live agents".into(),
            ),
            AxiError::Timeout => (
                "wait exceeded the timeout".into(),
                "raise --timeout or check the agent's state with `herdr-axi agents`".into(),
            ),
            AxiError::Other(raw) => (
                raw.clone(),
                "re-run with the raw herdr CLI to investigate".into(),
            ),
        }
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

/// Return the menu choice for a one-time Codex onboarding prompt, if present.
/// Directory trust is accepted, while hooks are deliberately left untrusted.
/// Pure text classification; independent of any backend.
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

/// Backend-agnostic pane/agent I/O contract. A concrete backend (e.g. the
/// herdr multiplexer) implements this against its own process/wire format;
/// the dispatch orchestration below is written generically against it.
pub trait Backend {
    /// List live agents.
    fn agents(&self) -> Result<Vec<Agent>, AxiError>;
    /// Type text into a pane WITHOUT submitting it.
    fn send_text(&self, pane: &str, text: &str) -> Result<(), AxiError>;
    /// Send a single named key (e.g. "enter", "escape") to a pane.
    fn send_key(&self, pane: &str, key: &str) -> Result<(), AxiError>;
    /// Read a pane's current visible content (used for onboarding detection).
    fn read_pane(&self, pane: &str) -> Result<String, AxiError>;
    /// Wait for an agent to reach a state (backend's native wait primitive).
    fn wait(
        &self,
        name: &str,
        until: Option<&str>,
        timeout_ms: Option<u64>,
    ) -> Result<String, AxiError>;
}

/// Submit a task to an agent using firstmate's verified-submit dance
/// (bin/backends/herdr.sh: fm_backend_herdr_send_text_submit): type once via
/// `send_text` (never retyped), then retry a single "enter" key press until
/// the agent's native state leaves idle (idle -> working transition = the
/// TUI accepted the submission), then wait for the settled state via the
/// backend's `wait`. A blocked agent is refused up-front, mirroring a
/// backend's own pre-check for agent_blocked. Written generically against
/// `Backend` so any implementation (herdr today, tmux later) gets identical
/// orchestration for free.
pub fn dispatch<B: Backend>(
    backend: &B,
    name: &str,
    task: &str,
    timeout_ms: u64,
) -> Result<String, AxiError> {
    let agents = backend.agents()?;
    let agent = agents
        .iter()
        .find(|a| a.name == name)
        .ok_or(AxiError::UnknownAgent)?;
    let pane = agent.pane_id.clone();
    let is_codex = agent.agent == "codex";

    if is_codex {
        clear_codex_onboarding(backend, &pane)?;
    }

    // Onboarding changes the native state reported by the backend.
    let agents = backend.agents()?;
    let agent = agents
        .iter()
        .find(|a| a.name == name)
        .ok_or(AxiError::UnknownAgent)?;

    // blocked agents self-heal: one Escape dismisses the dialog (firstmate's
    // composer-clear move); only a still-blocked agent surfaces as an error.
    if agent.agent_status == "blocked" {
        backend.send_key(&pane, "escape")?;
        std::thread::sleep(Duration::from_millis(1500));
        let still_blocked = backend.agents()?.iter().any(|a| {
            a.name == name && (a.agent_status == "blocked" || a.agent_status == "unknown")
        });
        if still_blocked {
            return Err(AxiError::Blocked);
        }
    }

    // 1. type the text once, unsubmitted
    backend.send_text(&pane, task)?;

    // 2. settle so completion popups / TUI redraws cannot swallow the Enter
    std::thread::sleep(Duration::from_millis(400));

    // 3. retry Enter only, watching for the idle -> working transition
    let mut accepted = agent.agent_status == "working";
    for _ in 0..3 {
        backend.send_key(&pane, "enter")?;
        if wait_for_working(backend, name, 5_000)? {
            accepted = true;
            break;
        }
    }
    if !accepted {
        return Err(AxiError::Stalled);
    }

    // 4. wait for the settled state (backend default: idle, done, or blocked)
    backend.wait(name, None, Some(timeout_ms))
}

/// Clear Codex's one-time onboarding chain before normal dispatch. The cap
/// prevents an unexpected pane from causing an unbounded interaction loop.
fn clear_codex_onboarding<B: Backend>(backend: &B, pane: &str) -> Result<(), AxiError> {
    for _ in 0..5 {
        let content = backend.read_pane(pane)?;
        let Some(choice) = codex_onboarding_choice(&content) else {
            break;
        };
        backend.send_key(pane, choice)?;
        backend.send_key(pane, "enter")?;
        std::thread::sleep(Duration::from_millis(400));
    }
    Ok(())
}

/// Poll agent state until `name` reports working (submission accepted).
/// Samples ~every 300ms across the budget; a transition landing partway
/// through is still caught (firstmate's wait_for_working pattern).
fn wait_for_working<B: Backend>(backend: &B, name: &str, budget_ms: u64) -> Result<bool, AxiError> {
    let deadline = Instant::now() + Duration::from_millis(budget_ms);
    while Instant::now() < deadline {
        if let Ok(agents) = backend.agents() {
            let working = agents
                .iter()
                .any(|a| a.name == name && a.agent_status == "working");
            if working {
                return Ok(true);
            }
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    Ok(false)
}
