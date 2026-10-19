//! `torrentx setup`: register `torrentx mcp` with the agents on this machine.
//!
//! Modeled on REA's setup: detect installed agents, show every change,
//! apply only after approval, and back up a file before editing it. An
//! agent that ships its own CLI for MCP registration (Claude Code, Codex)
//! is configured through that CLI, because the agent itself also writes its
//! configuration file while it runs; the others get their JSON file edited.

use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

pub const SERVER_NAME: &str = "torrentx";

#[derive(Debug, Clone)]
pub struct Client {
    pub id: &'static str,
    pub name: &'static str,
    /// The client is installed when this path exists.
    marker: PathBuf,
    method: Method,
}

#[derive(Debug, Clone)]
enum Method {
    /// The agent's own CLI, with the JSON file as the fallback when the CLI
    /// is not on PATH.
    Cli {
        program: &'static str,
        add: fn(&Path) -> Vec<String>,
        remove: &'static [&'static str],
        fallback: PathBuf,
    },
    /// A JSON file with an `mcpServers` object.
    Json(PathBuf),
}

/// One change setup proposes.
#[derive(Debug)]
pub enum Change {
    Run {
        client: &'static str,
        program: PathBuf,
        arguments: Vec<String>,
        /// Run first and ignore its failure: removes an earlier registration.
        before: Option<Vec<String>>,
    },
    EditJson {
        client: &'static str,
        path: PathBuf,
        /// None removes the entry.
        entry: Option<Value>,
    },
    Unchanged {
        client: &'static str,
        reason: String,
    },
}

impl std::fmt::Display for Change {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Run {
                client,
                program,
                arguments,
                ..
            } => write!(
                f,
                "{client}: run {} {}",
                program.display(),
                arguments.join(" ")
            ),
            Self::EditJson {
                client,
                path,
                entry: Some(_),
            } => write!(
                f,
                "{client}: add \"{SERVER_NAME}\" to mcpServers in {} (backup kept)",
                path.display()
            ),
            Self::EditJson {
                client,
                path,
                entry: None,
            } => write!(
                f,
                "{client}: remove \"{SERVER_NAME}\" from mcpServers in {} (backup kept)",
                path.display()
            ),
            Self::Unchanged { client, reason } => write!(f, "{client}: {reason}"),
        }
    }
}

fn claude_add(exe: &Path) -> Vec<String> {
    vec![
        "mcp".into(),
        "add".into(),
        "--scope".into(),
        "user".into(),
        SERVER_NAME.into(),
        "--".into(),
        exe.display().to_string(),
        "mcp".into(),
    ]
}

fn codex_add(exe: &Path) -> Vec<String> {
    vec![
        "mcp".into(),
        "add".into(),
        SERVER_NAME.into(),
        "--".into(),
        exe.display().to_string(),
        "mcp".into(),
    ]
}

/// Every agent setup knows, whether or not it is installed.
pub fn clients(home: &Path, app_data: Option<&Path>) -> Vec<Client> {
    let app_data = app_data
        .map(Path::to_path_buf)
        .unwrap_or_else(|| home.join("AppData").join("Roaming"));
    let claude_desktop = if cfg!(windows) {
        app_data.join("Claude")
    } else if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Claude")
    } else {
        home.join(".config/Claude")
    };
    vec![
        Client {
            id: "claude_code",
            name: "Claude Code",
            marker: home.join(".claude"),
            method: Method::Cli {
                program: "claude",
                add: claude_add,
                remove: &["mcp", "remove", "--scope", "user", SERVER_NAME],
                fallback: home.join(".claude.json"),
            },
        },
        Client {
            id: "codex",
            name: "Codex",
            marker: home.join(".codex"),
            method: Method::Cli {
                program: "codex",
                add: codex_add,
                remove: &["mcp", "remove", SERVER_NAME],
                fallback: PathBuf::new(),
            },
        },
        Client {
            id: "antigravity",
            name: "Antigravity",
            marker: home.join(".gemini/config"),
            method: Method::Json(home.join(".gemini/config/mcp_config.json")),
        },
        Client {
            id: "claude_desktop",
            name: "Claude Desktop",
            marker: claude_desktop.clone(),
            method: Method::Json(claude_desktop.join("claude_desktop_config.json")),
        },
        Client {
            id: "gemini_cli",
            name: "Gemini CLI",
            marker: home.join(".gemini/settings.json"),
            method: Method::Json(home.join(".gemini/settings.json")),
        },
        Client {
            id: "cursor",
            name: "Cursor",
            marker: home.join(".cursor"),
            method: Method::Json(home.join(".cursor/mcp.json")),
        },
    ]
}

pub fn installed(clients: Vec<Client>) -> Vec<Client> {
    clients.into_iter().filter(|c| c.marker.exists()).collect()
}

/// The registration every client gets.
pub fn entry(exe: &Path) -> Value {
    json!({"command": exe.display().to_string(), "args": ["mcp"]})
}

/// What setup would do for these clients; nothing is changed yet.
pub fn plan(clients: &[Client], exe: &Path, remove: bool) -> Vec<Change> {
    clients
        .iter()
        .map(|client| match &client.method {
            Method::Cli {
                program,
                add,
                remove: remove_arguments,
                fallback,
            } => match find_program(program) {
                Some(program) => {
                    let removal: Vec<String> =
                        remove_arguments.iter().map(|a| a.to_string()).collect();
                    if remove {
                        Change::Run {
                            client: client.name,
                            program,
                            arguments: removal,
                            before: None,
                        }
                    } else {
                        Change::Run {
                            client: client.name,
                            program,
                            arguments: add(exe),
                            before: Some(removal),
                        }
                    }
                }
                None if !fallback.as_os_str().is_empty() => {
                    json_change(client.name, fallback, exe, remove)
                }
                None => Change::Unchanged {
                    client: client.name,
                    reason: format!(
                        "`{program}` is not on PATH; register manually (see docs/mcp.md)"
                    ),
                },
            },
            Method::Json(path) => json_change(client.name, path, exe, remove),
        })
        .collect()
}

fn json_change(client: &'static str, path: &Path, exe: &Path, remove: bool) -> Change {
    let current = read_json(path).ok().flatten();
    let existing = current
        .as_ref()
        .and_then(|v| v.get("mcpServers"))
        .and_then(|s| s.get(SERVER_NAME));
    let wanted = (!remove).then(|| entry(exe));
    if existing == wanted.as_ref() {
        return Change::Unchanged {
            client,
            reason: if remove {
                "not registered".into()
            } else {
                "already registered".into()
            },
        };
    }
    Change::EditJson {
        client,
        path: path.into(),
        entry: wanted,
    }
}

fn read_json(path: &Path) -> Result<Option<Value>, String> {
    match std::fs::read(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("{}: {error}", path.display())),
        Ok(bytes) if bytes.iter().all(u8::is_ascii_whitespace) => Ok(None),
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            serde_json::from_str(text.trim_start_matches('\u{feff}'))
                .map(Some)
                .map_err(|error| format!("{} is not valid JSON: {error}", path.display()))
        }
    }
}

/// Apply one change; returns what happened, for the report.
pub fn apply(change: &Change) -> Result<String, String> {
    match change {
        Change::Unchanged { client, reason } => Ok(format!("{client}: {reason}")),
        Change::Run {
            client,
            program,
            arguments,
            before,
        } => {
            if let Some(before) = before {
                let _ = Command::new(program).args(before).output();
            }
            let output = Command::new(program)
                .args(arguments)
                .output()
                .map_err(|error| format!("{client}: {error}"))?;
            if output.status.success() {
                Ok(format!("{client}: done"))
            } else {
                Err(format!(
                    "{client}: {} failed: {}",
                    program.display(),
                    String::from_utf8_lossy(&output.stderr).trim()
                ))
            }
        }
        Change::EditJson {
            client,
            path,
            entry,
        } => {
            let mut document = read_json(path)?.unwrap_or_else(|| json!({}));
            let Value::Object(root) = &mut document else {
                return Err(format!("{client}: {} is not a JSON object", path.display()));
            };
            let servers = root
                .entry("mcpServers")
                .or_insert_with(|| json!({}));
            let Value::Object(servers) = servers else {
                return Err(format!(
                    "{client}: mcpServers in {} is not an object",
                    path.display()
                ));
            };
            match entry {
                Some(entry) => {
                    servers.insert(SERVER_NAME.into(), entry.clone());
                }
                None => {
                    servers.remove(SERVER_NAME);
                }
            }
            if path.exists() {
                let backup = backup_path(path);
                std::fs::copy(path, &backup)
                    .map_err(|error| format!("{client}: backup failed: {error}"))?;
            } else if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|error| format!("{client}: {error}"))?;
            }
            let mut text = serde_json::to_string_pretty(&document).map_err(|e| e.to_string())?;
            text.push('\n');
            // Write then rename, so the agent never reads half a file.
            let partial = path.with_extension("torrentx-partial");
            std::fs::write(&partial, text).map_err(|error| format!("{client}: {error}"))?;
            std::fs::rename(&partial, path).map_err(|error| {
                let _ = std::fs::remove_file(&partial);
                format!("{client}: {error}")
            })?;
            Ok(format!("{client}: done"))
        }
    }
}

fn backup_path(path: &Path) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!("{name}.torrentx-backup-{stamp}"))
}

/// The program as Windows or Unix would run it from PATH.
fn find_program(name: &str) -> Option<PathBuf> {
    let extensions: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };
    std::env::split_paths(&std::env::var_os("PATH")?).find_map(|directory| {
        extensions
            .iter()
            .map(|extension| directory.join(format!("{name}{extension}")))
            .find(|candidate| candidate.is_file())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json_client(path: &Path) -> Client {
        Client {
            id: "test",
            name: "Test",
            marker: path.parent().unwrap().into(),
            method: Method::Json(path.into()),
        }
    }

    #[test]
    fn adds_keeps_other_servers_backs_up_and_removes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("mcp_config.json");
        std::fs::write(
            &path,
            r#"{"theme": "dark", "mcpServers": {"rea": {"command": "node", "args": ["rea.mjs", "mcp"]}}}"#,
        )
        .unwrap();
        let exe = Path::new(r"C:\Tools\torrentx\torrentx.exe");
        let client = json_client(&path);

        let changes = plan(std::slice::from_ref(&client), exe, false);
        assert!(matches!(changes[0], Change::EditJson { entry: Some(_), .. }));
        apply(&changes[0]).unwrap();
        let written: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written["theme"], "dark");
        assert_eq!(written["mcpServers"]["rea"]["command"], "node");
        assert_eq!(written["mcpServers"][SERVER_NAME], entry(exe));
        let backups = std::fs::read_dir(directory.path())
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .contains("torrentx-backup")
            })
            .count();
        assert_eq!(backups, 1);

        // Running setup again changes nothing.
        let again = plan(std::slice::from_ref(&client), exe, false);
        assert!(matches!(again[0], Change::Unchanged { .. }));

        let removal = plan(std::slice::from_ref(&client), exe, true);
        apply(&removal[0]).unwrap();
        let written: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(written["mcpServers"].get(SERVER_NAME).is_none());
        assert_eq!(written["mcpServers"]["rea"]["command"], "node");
    }

    #[test]
    fn creates_a_missing_file_and_refuses_a_broken_one() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("new/mcp.json");
        let exe = Path::new("/opt/torrentx/torrentx");
        let client = json_client(&path);
        apply(&plan(std::slice::from_ref(&client), exe, false)[0]).unwrap();
        let written: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written["mcpServers"][SERVER_NAME]["args"], json!(["mcp"]));

        std::fs::write(&path, "{ not json").unwrap();
        let change = Change::EditJson {
            client: "Test",
            path: path.clone(),
            entry: Some(entry(exe)),
        };
        assert!(apply(&change).unwrap_err().contains("not valid JSON"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
    }

    #[test]
    fn knows_where_each_agent_registers() {
        let home = Path::new("/home/u");
        let all = clients(home, Some(Path::new("/appdata")));
        let ids: Vec<&str> = all.iter().map(|c| c.id).collect();
        assert_eq!(
            ids,
            ["claude_code", "codex", "antigravity", "claude_desktop", "gemini_cli", "cursor"]
        );
        let antigravity = all.iter().find(|c| c.id == "antigravity").unwrap();
        assert!(
            matches!(&antigravity.method, Method::Json(p) if p.ends_with(".gemini/config/mcp_config.json"))
        );
    }
}
