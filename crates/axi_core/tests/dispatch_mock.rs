//! Deterministic mock-Backend coverage for `axi_core::dispatch`'s
//! orchestration: type-once/settle, Enter-retry confirmed by
//! idle->working, settled-state wait, blocked-pane self-heal, unknown
//! agent, and Codex onboarding clearing. No process spawning, no herdr —
//! a fake in-memory `Backend` impl only. This is mock coverage, not live
//! evidence (see VERIFICATION-DISPATCH-20260905.md for the real-pane run).

use axi_core::{Agent, AxiError, Backend, dispatch};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;

/// Scripted in-memory `Backend`. Every call is recorded so tests can assert
/// on the exact sequence of pane operations dispatch() performed, and the
/// agent's reported status can be mutated by `send_key`/`send_text` to
/// simulate real backend state transitions deterministically (no sleeps
/// tied to wall clock content, only the orchestration's own timing).
struct FakeBackend {
    name: &'static str,
    kind: &'static str,
    pane: &'static str,
    status: Cell<&'static str>,
    /// becomes "working" once this many `send_key("enter")` calls landed;
    /// None means it never transitions (stalled-path simulation).
    enter_to_work_at: Option<u32>,
    enter_count: Cell<u32>,
    /// if true, a single `send_key("escape")` clears "blocked" -> "idle".
    escape_heals: bool,
    /// scripted pane contents returned by `read_pane`, consumed in order;
    /// once empty, "" (no more onboarding prompts) is returned.
    onboarding_screens: RefCell<VecDeque<&'static str>>,
    wait_result: RefCell<Option<Result<String, AxiError>>>,

    agents_calls: Cell<u32>,
    send_text_calls: RefCell<Vec<(String, String)>>,
    send_key_calls: RefCell<Vec<(String, String)>>,
    read_pane_calls: Cell<u32>,
    wait_calls: Cell<u32>,
}

impl FakeBackend {
    fn new(
        name: &'static str,
        kind: &'static str,
        pane: &'static str,
        status: &'static str,
    ) -> Self {
        FakeBackend {
            name,
            kind,
            pane,
            status: Cell::new(status),
            enter_to_work_at: Some(1),
            enter_count: Cell::new(0),
            escape_heals: false,
            onboarding_screens: RefCell::new(VecDeque::new()),
            wait_result: RefCell::new(Some(Ok("idle".to_string()))),
            agents_calls: Cell::new(0),
            send_text_calls: RefCell::new(Vec::new()),
            send_key_calls: RefCell::new(Vec::new()),
            read_pane_calls: Cell::new(0),
            wait_calls: Cell::new(0),
        }
    }

    fn never_works(mut self) -> Self {
        self.enter_to_work_at = None;
        self
    }

    fn heals_on_escape(mut self) -> Self {
        self.escape_heals = true;
        self
    }

    fn with_onboarding(self, screens: &[&'static str]) -> Self {
        *self.onboarding_screens.borrow_mut() = screens.iter().copied().collect();
        self
    }
}

impl Backend for FakeBackend {
    fn agents(&self) -> Result<Vec<Agent>, AxiError> {
        self.agents_calls.set(self.agents_calls.get() + 1);
        Ok(vec![Agent {
            name: self.name.to_string(),
            agent: self.kind.to_string(),
            pane_id: self.pane.to_string(),
            agent_status: self.status.get().to_string(),
        }])
    }

    fn send_text(&self, pane: &str, text: &str) -> Result<(), AxiError> {
        self.send_text_calls
            .borrow_mut()
            .push((pane.to_string(), text.to_string()));
        Ok(())
    }

    fn send_key(&self, pane: &str, key: &str) -> Result<(), AxiError> {
        self.send_key_calls
            .borrow_mut()
            .push((pane.to_string(), key.to_string()));
        if key == "enter" {
            let n = self.enter_count.get() + 1;
            self.enter_count.set(n);
            if self.status.get() != "working"
                && let Some(threshold) = self.enter_to_work_at
                && n >= threshold
            {
                self.status.set("working");
            }
        } else if key == "escape" && self.escape_heals && self.status.get() == "blocked" {
            self.status.set("idle");
        }
        Ok(())
    }

    fn read_pane(&self, _pane: &str) -> Result<String, AxiError> {
        self.read_pane_calls.set(self.read_pane_calls.get() + 1);
        Ok(self
            .onboarding_screens
            .borrow_mut()
            .pop_front()
            .unwrap_or_default()
            .to_string())
    }

    fn wait(
        &self,
        _name: &str,
        _until: Option<&str>,
        _timeout_ms: Option<u64>,
    ) -> Result<String, AxiError> {
        self.wait_calls.set(self.wait_calls.get() + 1);
        self.wait_result
            .borrow_mut()
            .take()
            .unwrap_or(Ok("idle".to_string()))
    }
}

#[test]
fn dispatch_happy_path_idle_to_working_to_settled() {
    let backend = FakeBackend::new("bob", "claude", "%1", "idle");
    let result = dispatch(&backend, "bob", "do the thing", 30_000);

    assert_eq!(result, Ok("idle".to_string()));
    assert_eq!(
        backend.send_text_calls.borrow().as_slice(),
        &[("%1".to_string(), "do the thing".to_string())]
    );
    // exactly one Enter should have been needed (enter_to_work_at = 1)
    assert_eq!(backend.enter_count.get(), 1);
    assert_eq!(backend.wait_calls.get(), 1);
    assert_eq!(backend.status.get(), "working");
    // no onboarding, no escape sent
    assert!(
        !backend
            .send_key_calls
            .borrow()
            .iter()
            .any(|(_, k)| k == "escape")
    );
}

#[test]
fn dispatch_unknown_agent_returns_error() {
    let backend = FakeBackend::new("bob", "claude", "%1", "idle");
    let result = dispatch(&backend, "ghost", "hi", 30_000);

    assert_eq!(result, Err(AxiError::UnknownAgent));
    // failed at the first agents() lookup; no pane I/O should have happened
    assert!(backend.send_text_calls.borrow().is_empty());
    assert!(backend.send_key_calls.borrow().is_empty());
    assert_eq!(backend.wait_calls.get(), 0);
}

#[test]
fn dispatch_blocked_agent_self_heals_via_escape_then_proceeds() {
    let backend = FakeBackend::new("bob", "claude", "%1", "blocked").heals_on_escape();
    let result = dispatch(&backend, "bob", "do the thing", 30_000);

    assert_eq!(result, Ok("idle".to_string()));
    assert_eq!(
        backend.send_key_calls.borrow().first(),
        Some(&("%1".to_string(), "escape".to_string()))
    );
    // after healing to idle, the normal type/settle/enter/wait path ran
    assert_eq!(
        backend.send_text_calls.borrow().as_slice(),
        &[("%1".to_string(), "do the thing".to_string())]
    );
    assert_eq!(backend.wait_calls.get(), 1);
}

#[test]
fn dispatch_blocked_agent_stays_blocked_returns_error() {
    // escape_heals=false (default): the agent never leaves "blocked".
    let backend = FakeBackend::new("bob", "claude", "%1", "blocked");
    let result = dispatch(&backend, "bob", "do the thing", 30_000);

    assert_eq!(result, Err(AxiError::Blocked));
    // dispatch must bail out before ever typing text into a blocked pane
    assert!(backend.send_text_calls.borrow().is_empty());
    assert_eq!(backend.wait_calls.get(), 0);
}

#[test]
fn dispatch_clears_codex_onboarding_before_dispatching() {
    let backend = FakeBackend::new("bob", "codex", "%1", "idle").with_onboarding(&[
        "Do you trust the contents of this directory?",
        "Continue without trusting hooks",
    ]);
    let result = dispatch(&backend, "bob", "do the thing", 30_000);

    assert_eq!(result, Ok("idle".to_string()));
    assert_eq!(backend.read_pane_calls.get(), 3); // 2 prompts + 1 "ready" check
    // choice "1" (trust directory) then "3" (skip hooks trust), each Enter-confirmed
    let keys: Vec<String> = backend
        .send_key_calls
        .borrow()
        .iter()
        .map(|(_, k)| k.clone())
        .collect();
    assert_eq!(&keys[..4], &["1", "enter", "3", "enter"]);
    // normal dispatch still ran after onboarding cleared
    assert_eq!(
        backend.send_text_calls.borrow().as_slice(),
        &[("%1".to_string(), "do the thing".to_string())]
    );
}

// Slow-by-construction: exercises the genuine stalled path, i.e. 3 real
// Enter retries each exhausting axi_core's real 5s wait_for_working budget
// (no injectable clock in the extracted orchestration). ~15s wall time;
// kept as its own test so `cargo test` output makes the cost visible.
#[test]
fn dispatch_stalled_when_agent_never_leaves_idle() {
    let backend = FakeBackend::new("bob", "claude", "%1", "idle").never_works();
    let result = dispatch(&backend, "bob", "do the thing", 30_000);

    assert_eq!(result, Err(AxiError::Stalled));
    assert_eq!(backend.enter_count.get(), 3); // all 3 retries attempted
    assert_eq!(backend.wait_calls.get(), 0); // never reached the settle wait
}
