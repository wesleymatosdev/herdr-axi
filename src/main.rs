//! herdr-axi CLI surface. No interactive prompts, ever.

use clap::{Parser, Subcommand};
use herdr_axi::{dispatch, live_agents, wait as herdr_wait};

#[derive(Parser)]
#[command(
    name = "herdr-axi",
    version,
    about = "AXI-discipline wrapper around the herdr multiplexer CLI"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List live agents (name, kind, pane, state)
    Agents {
        /// Emit machine-readable JSON instead of the human table
        #[arg(long)]
        json: bool,
    },
    /// Submit a task to an agent and wait for the next settled state
    Dispatch {
        /// Agent name
        name: String,
        /// Task text (rest of the line)
        task: Vec<String>,
        /// Timeout in milliseconds
        #[arg(long, default_value_t = 300_000)]
        timeout: u64,
    },
    /// Wait until an agent reaches a state
    Wait {
        /// Agent name
        name: String,
        /// State to wait for (repeatable upstream; v0.1 takes one)
        #[arg(long)]
        until: Option<String>,
        /// Timeout in milliseconds
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// Fleet aggregate: counts by state + names of blocked agents
    Fleet {
        /// Emit machine-readable JSON instead of the aggregate line
        #[arg(long)]
        json: bool,
    },
}

fn print_err(e: herdr_axi::HerdrError) -> i32 {
    let (cause, action) = e.cause_action();
    eprintln!("error: {cause}\n  action: {action}");
    e.exit_code()
}

fn main() {
    let cli = Cli::parse();
    let code = match cli.cmd {
        Cmd::Agents { json } => match live_agents() {
            Ok(agents) => {
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&agents_json(&agents))
                            .unwrap_or_else(|_| "[]".into())
                    );
                } else {
                    print!("{}", herdr_axi::format_agents_table(&agents));
                }
                0
            }
            Err(e) => print_err(e),
        },
        Cmd::Dispatch {
            name,
            task,
            timeout,
        } => {
            let task = task.join(" ");
            if task.is_empty() {
                eprintln!(
                    "error: dispatch needs a task\n  action: pass the task text after the agent name"
                );
                std::process::exit(1);
            }
            match dispatch(&name, &task, timeout) {
                Ok(out) => {
                    println!("{out}");
                    0
                }
                Err(e) => print_err(e),
            }
        }
        Cmd::Wait {
            name,
            until,
            timeout,
        } => match herdr_wait(&name, until.as_deref(), timeout) {
            Ok(out) => {
                println!("{out}");
                0
            }
            Err(e) => print_err(e),
        },
        Cmd::Fleet { json } => match live_agents() {
            Ok(agents) => {
                if json {
                    let counts = herdr_axi::fleet_counts(&agents);
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "counts": counts,
                            "blocked": agents
                                .iter()
                                .filter(|a| a.agent_status == "blocked")
                                .map(|a| a.name.clone())
                                .collect::<Vec<_>>(),
                        }))
                        .unwrap_or_else(|_| "{}".into())
                    );
                } else {
                    println!("{}", herdr_axi::format_fleet(&agents));
                }
                0
            }
            Err(e) => print_err(e),
        },
    };
    std::process::exit(code);
}

fn agents_json(agents: &[herdr_axi::Agent]) -> serde_json::Value {
    serde_json::json!(
        agents
            .iter()
            .map(|a| serde_json::json!({
                "name": a.name,
                "kind": a.agent,
                "pane_id": a.pane_id,
                "state": a.agent_status,
            }))
            .collect::<Vec<_>>()
    )
}
