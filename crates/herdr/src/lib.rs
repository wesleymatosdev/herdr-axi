//! herdr backend: process spawning against the real `herdr` multiplexer CLI,
//! plus translation of herdr's own JSON wire error shapes into
//! `axi_core::AxiError` at the boundary. herdr's error shapes are herdr's
//! problem; nothing about them leaks into axi_core.

use axi_core::{Agent, AxiError, Backend};
use std::process::Command;

/// Where the herdr binary lives.
pub const HERDR_BIN: &str = "~/.local/bin/herdr";

fn herdr_path() -> String {
    if let Ok(home) = std::env::var("HOME") {
        return format!("{home}/.local/bin/herdr");
    }
    HERDR_BIN.to_string()
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
pub fn parse_error(stdout: &str) -> AxiError {
    let trimmed = stdout.trim();
    let v: serde_json::Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(_) => return AxiError::Other(trimmed.to_string()),
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
        "agent_blocked" => AxiError::Blocked,
        "agent_prompt_stalled" => AxiError::Stalled,
        "agent_not_found" | "unknown_agent" => AxiError::UnknownAgent,
        "timeout" => AxiError::Timeout,
        _ => AxiError::Other(trimmed.to_string()),
    }
}

/// Run a herdr subcommand, mapping failure through parse_error.
fn run_herdr(args: &[&str]) -> Result<String, AxiError> {
    let out = Command::new(herdr_path())
        .args(args)
        .output()
        .map_err(|e| AxiError::Other(format!("failed to spawn herdr: {e}")))?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        return Err(parse_error(&format!("{stdout}{stderr}")));
    }
    Ok(stdout)
}

/// Run `herdr agent list` and parse it.
pub fn live_agents() -> Result<Vec<Agent>, AxiError> {
    let stdout = run_herdr(&["agent", "list"])?;
    Ok(parse_agent_list(&stdout))
}

/// The herdr multiplexer as an `axi_core::Backend`.
pub struct HerdrBackend;

impl Default for HerdrBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl HerdrBackend {
    pub fn new() -> Self {
        HerdrBackend
    }
}

impl Backend for HerdrBackend {
    fn agents(&self) -> Result<Vec<Agent>, AxiError> {
        live_agents()
    }

    fn send_text(&self, pane: &str, text: &str) -> Result<(), AxiError> {
        run_herdr(&["pane", "send-text", pane, text])?;
        Ok(())
    }

    fn send_key(&self, pane: &str, key: &str) -> Result<(), AxiError> {
        run_herdr(&["pane", "send-keys", pane, key])?;
        Ok(())
    }

    fn read_pane(&self, pane: &str) -> Result<String, AxiError> {
        run_herdr(&[
            "pane",
            "read",
            pane,
            "--source",
            "detection",
            "--format",
            "text",
        ])
    }

    fn wait(
        &self,
        name: &str,
        until: Option<&str>,
        timeout_ms: Option<u64>,
    ) -> Result<String, AxiError> {
        let mut args = vec!["agent", "wait", name];
        if let Some(u) = until {
            args.push("--until");
            args.push(u);
        }
        let timeout_str;
        if let Some(t) = timeout_ms {
            timeout_str = t.to_string();
            args.push("--timeout");
            args.push(&timeout_str);
        }
        let out = Command::new(herdr_path())
            .args(&args)
            .output()
            .map_err(|e| AxiError::Other(format!("failed to spawn herdr: {e}")))?;
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            return Err(parse_error(&format!("{stdout}{stderr}")));
        }
        Ok(stdout.trim().to_string())
    }
}

/// Submit a task to an agent through the herdr backend and wait for the
/// next settled state. Thin convenience wrapper around
/// `axi_core::dispatch` bound to `HerdrBackend`, kept so the CLI can call a
/// free function exactly as before.
pub fn dispatch(name: &str, task: &str, timeout_ms: u64) -> Result<String, AxiError> {
    axi_core::dispatch(&HerdrBackend::new(), name, task, timeout_ms)
}

/// Wait for an agent to reach a state via `herdr agent wait`.
pub fn wait(name: &str, until: Option<&str>, timeout_ms: Option<u64>) -> Result<String, AxiError> {
    HerdrBackend::new().wait(name, until, timeout_ms)
}
