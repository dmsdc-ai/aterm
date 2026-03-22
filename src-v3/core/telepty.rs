use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;

use super::pty::resolve_command_binary;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeleptySessionInfo {
    pub id: String,
    pub cwd: String,
    pub command: String,
    pub host: String,
    pub clients: usize,
    pub started_at: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct TeleptyClient {
    binary: PathBuf,
}

impl Default for TeleptyClient {
    fn default() -> Self {
        Self::new()
    }
}

impl TeleptyClient {
    pub fn new() -> Self {
        Self {
            binary: resolve_command_binary("telepty"),
        }
    }

    pub fn with_binary(binary: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
        }
    }

    pub fn list_sessions(&self) -> Result<Vec<TeleptySessionInfo>, String> {
        let path_env = super::pty::augmented_path_env();
        let run_list = |args: &[&str]| {
            let mut command = Command::new(&self.binary);
            command.args(args);
            if let Some(path_env) = path_env.as_ref() {
                command.env("PATH", path_env);
            }
            command.output()
        };

        for args in [&["list", "--json"][..], &["list"][..]] {
            let Ok(output) = run_list(args) else {
                continue;
            };
            if !output.status.success() && args.len() == 2 && args[0] == "list" && args[1] == "--json" {
                continue;
            }

            let stdout = String::from_utf8_lossy(&output.stdout);
            let trimmed = stdout.trim();

            if trimmed.starts_with('[') {
                if let Ok(sessions) = serde_json::from_str::<Vec<TeleptySessionInfo>>(trimmed) {
                    return Ok(sessions);
                }
            }

            if trimmed.starts_with('{') {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct Envelope {
                    sessions: Vec<TeleptySessionInfo>,
                }

                if let Ok(envelope) = serde_json::from_str::<Envelope>(trimmed) {
                    return Ok(envelope.sessions);
                }
            }

            if !trimmed.is_empty() || output.status.success() {
                return Ok(parse_telepty_sessions_text(trimmed));
            }
        }

        Err(format!(
            "failed to launch telepty binary: {}",
            self.binary.display()
        ))
    }

    pub fn inject(&self, from: &str, target: &str, text: &str) -> Result<(), String> {
        let path_env = super::pty::augmented_path_env();
        let mut command = Command::new(&self.binary);
        command.args(["inject", "--from", from, target, text]);
        if let Some(path_env) = path_env.as_ref() {
            command.env("PATH", path_env);
        }

        let output = command.output().map_err(|error| error.to_string())?;
        if output.status.success() {
            return Ok(());
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(if stderr.trim().is_empty() {
            format!("telepty inject failed with status {}", output.status)
        } else {
            stderr.trim().to_string()
        })
    }
}

fn parse_telepty_sessions_text(raw: &str) -> Vec<TeleptySessionInfo> {
    let cleaned = strip_ansi(raw);
    let mut sessions: Vec<TeleptySessionInfo> = Vec::new();
    let mut current: Option<TeleptySessionInfo> = None;

    for line in cleaned.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed == "Active Sessions:" {
            continue;
        }

        if let Some(id) = trimmed.strip_prefix("- ID: ") {
            if let Some(session) = current.take() {
                sessions.push(session);
            }
            current = Some(TeleptySessionInfo {
                id: id.trim().to_string(),
                status: "active".to_string(),
                ..Default::default()
            });
            continue;
        }

        let Some(session) = current.as_mut() else {
            continue;
        };

        if let Some(host) = trimmed.strip_prefix("Host: ") {
            session.host = host.trim().to_string();
        } else if let Some(command) = trimmed.strip_prefix("Command: ") {
            session.command = command.trim().to_string();
        } else if let Some(cwd) = trimmed.strip_prefix("CWD: ") {
            session.cwd = cwd.trim().to_string();
        } else if let Some(clients) = trimmed.strip_prefix("Clients: ") {
            session.clients = clients.trim().parse::<usize>().unwrap_or(0);
        } else if let Some(started) = trimmed.strip_prefix("Started: ") {
            session.started_at = started.trim().to_string();
        }
    }

    if let Some(session) = current.take() {
        sessions.push(session);
    }

    sessions
}

fn strip_ansi(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut escaping = false;

    for ch in input.chars() {
        if escaping {
            if ch.is_ascii_alphabetic() || matches!(ch, '~' | '\\') {
                escaping = false;
            }
            continue;
        }

        if ch == '\u{1b}' {
            escaping = true;
            continue;
        }

        output.push(ch);
    }

    output
}
