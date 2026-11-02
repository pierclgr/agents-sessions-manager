//! the command-line contract shared by both installed command names

use crate::{
    data::{
        Agent, DataPaths, Session,
        agents::{load_session_names, load_session_summaries, provider},
    },
    search::{Candidate, calendar_period},
    tui,
    utils::{ProcessRunner, SHORT_ID_LENGTH, format_datetime, truncate},
};
use chrono::Local;
use std::{
    cmp::Reverse,
    collections::HashMap,
    env,
    io::{self, Write},
    os::unix::process::CommandExt,
    path::Path,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

const LIST_COMMAND_MAX: usize = 50;
const DELETE_COMMAND_MAX: usize = 60;
const LIST_USAGE: &str = "Usage: agents-sessions ls [-a | --all] [QUERY]";
const RESUME_USAGE: &str =
    "Usage: agents-sessions resume <ID | NAME | latest> [-a AGENT | --agent AGENT]";

const HELP: &str = "Usage: agents-sessions [COMMAND] [OPTIONS]

Agents Sessions Manager - Browse, resume, and delete coding agent sessions.
Supported agents: Claude Code, Codex, OpenCode, Pi.

Commands:
  (none)                       Launch the interactive session browser
  ls [QUERY]                   List or search sessions from the current directory
                               Use -a or --all to list every directory
  resume <ID | NAME | latest>   Resume by ID prefix or exact name (any directory),
                               or the latest session in the current directory
  delete <ID>                  Delete a session and all its data (any agent)

Options:
  -a, --all                    With 'ls', include sessions from every directory
  -a, --agent AGENT            With 'resume', filter by claude, codex, opencode, or pi
  -h, --help                   Show this help message and exit

Examples:
  agents-sessions                         # Open interactive browser
  agents-sessions ls                      # List sessions from the current directory
  agents-sessions ls --all                # List sessions from all directories
  agents-sessions ls today                # Search sessions from the current directory
  agents-sessions ls --all \"last week\"    # Search sessions from all directories
  agents-sessions resume abc123           # Resume session abc123
  agents-sessions resume \"Fix login bug\"  # Resume by name (case-insensitive)
  agents-sessions resume latest           # Resume the latest session in this directory
  agents-sessions resume latest -a codex  # Resume the latest Codex session here
  agents-sessions delete abc123           # Delete session abc123
";

pub fn run() -> Result<(), String> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let command = args
        .first()
        .map(|arg| arg.to_string_lossy().into_owned())
        .unwrap_or_default();
    if matches!(command.as_str(), "-h" | "--help") {
        print!("{HELP}");
        return Ok(());
    }
    if command == "delete" && args.get(1).is_none_or(|id| id.is_empty()) {
        return Err(format!(
            "Error: '{command}' requires a session ID argument.\nUsage: agents-sessions {command} <SESSION_ID>"
        ));
    }
    if !matches!(command.as_str(), "" | "ls" | "resume" | "delete") {
        return Err(format!(
            "Error: Unknown command '{command}'.\nRun 'agents-sessions --help' for usage information."
        ));
    }
    let mut search_query = None;
    let mut list_all = false;
    let mut resume_target = None;
    let mut resume_agent = None;
    if command == "ls" {
        let mut options = true;
        for argument in &args[1..] {
            let argument = argument.to_string_lossy();
            if options {
                match argument.as_ref() {
                    "-a" | "--all" => {
                        list_all = true;
                        continue;
                    }
                    "--" => {
                        options = false;
                        continue;
                    }
                    value if value.starts_with('-') && value != "-" => {
                        return Err(format!(
                            "Error: Unknown option '{value}' for 'ls'.\n{LIST_USAGE}"
                        ));
                    }
                    _ => {}
                }
            }
            if search_query.is_some() {
                return Err(format!(
                    "Error: 'ls' accepts one query; quote text with spaces. Unexpected argument: '{argument}'.\n{LIST_USAGE}"
                ));
            }
            search_query = Some(argument.into_owned());
        }
    }
    if command == "resume" {
        let mut arguments = args[1..].iter();
        let mut options = true;
        while let Some(argument) = arguments.next() {
            let argument = argument.to_string_lossy();
            if options {
                let agent_name = match argument.as_ref() {
                    "--" => {
                        options = false;
                        continue;
                    }
                    "-a" | "--agent" => Some(
                        arguments
                            .next()
                            .ok_or_else(|| {
                                format!("Error: '{argument}' requires an agent argument.\n{RESUME_USAGE}")
                            })?
                            .to_string_lossy()
                            .into_owned(),
                    ),
                    value if value.starts_with("--agent=") => {
                        value.strip_prefix("--agent=").map(str::to_owned)
                    }
                    value if value.starts_with('-') && value != "-" => {
                        return Err(format!(
                            "Error: Unknown option '{value}' for 'resume'.\n{RESUME_USAGE}"
                        ));
                    }
                    _ => None,
                };
                if let Some(agent_name) = agent_name {
                    if resume_agent.is_some() {
                        return Err(format!(
                            "Error: 'resume' accepts one agent filter.\n{RESUME_USAGE}"
                        ));
                    }
                    resume_agent = Some(
                        Agent::ALL
                            .into_iter()
                            .find(|agent| agent.key() == agent_name)
                            .ok_or_else(|| {
                                format!(
                                    "Error: Unknown agent '{agent_name}'. Expected claude, codex, opencode, or pi.\n{RESUME_USAGE}"
                                )
                            })?,
                    );
                    continue;
                }
            }
            if resume_target.is_some() {
                return Err(format!(
                    "Error: 'resume' accepts one session ID or name; quote names with spaces. Unexpected argument: '{argument}'.\n{RESUME_USAGE}"
                ));
            }
            resume_target = Some(argument.into_owned());
        }
        if resume_target.as_ref().is_none_or(String::is_empty) {
            return Err(format!(
                "Error: 'resume' requires a session ID, name, or 'latest' argument.\n{RESUME_USAGE}"
            ));
        }
    }
    let paths = DataPaths::from_env()?;
    match command.as_str() {
        "" => {
            let sessions = load_session_summaries(&paths, None)?;
            if let Some(session) = tui::run(&paths, sessions)? {
                resume(&session, &paths)?;
            }
        }
        "ls" => {
            let mut sessions = load_session_summaries(&paths, None)?;
            if !list_all {
                let cwd = env::current_dir().map_err(|error| {
                    format!("Error: Cannot read the current directory: {error}")
                })?;
                retain_current_directory_sessions(&mut sessions, &cwd);
            }
            let indices: Vec<_> = (0..sessions.len()).collect();
            load_session_names(&mut sessions, &indices, &paths);
            if let Some(query) = search_query.filter(|query| !query.is_empty()) {
                let query = query.to_lowercase();
                let period = calendar_period(&query, Local::now().date_naive());
                sessions.retain(|session| Candidate::new(session).matches(&query, period.as_ref()));
            }
            list(&sessions);
        }
        "resume" => {
            let target = resume_target.as_deref().unwrap();
            let latest = target == "latest";
            let id_prefix = if latest { None } else { Some(target) };
            let load_sessions = |id_prefix: Option<&str>| {
                let Some(agent) = resume_agent else {
                    return load_session_summaries(&paths, id_prefix);
                };
                let mut sessions = provider(agent).load_summaries(&paths, id_prefix)?;
                sessions.sort_by_key(|session| Reverse(session.last_active_at));
                Ok(sessions)
            };
            let agent_suffix = resume_agent
                .map(|agent| format!(" for agent '{}'", agent.key()))
                .unwrap_or_default();
            let mut sessions = load_sessions(id_prefix)?;
            if latest {
                let cwd = env::current_dir().map_err(|error| {
                    format!("Error: Cannot read the current directory: {error}")
                })?;
                retain_current_directory_sessions(&mut sessions, &cwd);
                if sessions.is_empty() {
                    return Err(format!(
                        "Error: No sessions found in the current directory '{}'{}.",
                        cwd.display(),
                        agent_suffix
                    ));
                }
            } else if sessions.is_empty() {
                // keep ID lookups bounded; only load names when no ID prefix matches
                sessions = load_sessions(None)?;
                let indices: Vec<_> = (0..sessions.len()).collect();
                load_session_names(&mut sessions, &indices, &paths);
                let name = target.to_lowercase();
                sessions.retain(|session| {
                    session
                        .name
                        .as_ref()
                        .is_some_and(|session_name| session_name.to_lowercase() == name)
                });
            }
            let session = sessions
                .first()
                .ok_or_else(|| format!("Error: Session '{target}' not found{agent_suffix}."))?;
            resume(session, &paths)?;
        }
        _ => {
            let id = args[1].to_string_lossy();
            let mut sessions = load_session_summaries(&paths, Some(id.as_ref()))?;
            if sessions.is_empty() {
                return Err(format!("Error: Session '{id}' not found."));
            }
            load_session_names(&mut sessions, &[0], &paths);
            delete(&sessions[0], &paths)?;
        }
    }
    Ok(())
}

fn retain_current_directory_sessions(sessions: &mut Vec<Session>, cwd: &Path) {
    let canonical_cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let mut directory_matches = HashMap::new();
    sessions.retain(|session| {
        session.project_path == cwd
            || *directory_matches
                .entry(session.project_path.clone())
                .or_insert_with(|| {
                    session
                        .project_path
                        .canonicalize()
                        .is_ok_and(|path| path == canonical_cwd)
                })
    });
}

fn list(sessions: &[Session]) {
    if sessions.is_empty() {
        eprintln!("No sessions found.");
        return;
    }
    let headers = [
        "Agent",
        "Session ID",
        "Name",
        "Project Directory",
        "Last message",
        "Created",
        "Last Modified",
    ];
    let rows: Vec<_> = sessions
        .iter()
        .map(|session| {
            [
                session.agent.key().into(),
                session
                    .session_id
                    .chars()
                    .take(SHORT_ID_LENGTH)
                    .collect::<String>(),
                session.name.clone().unwrap_or_default(),
                session.project_path.display().to_string(),
                truncate(session.latest_command.trim(), LIST_COMMAND_MAX),
                format_datetime(session.created_at),
                format_datetime(session.last_active_at),
            ]
        })
        .collect();
    let mut widths = headers.map(str::len);
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    println!(
        "{}",
        headers
            .iter()
            .zip(widths)
            .map(|(cell, width)| format!("{cell:<width$}"))
            .collect::<Vec<_>>()
            .join("\t")
    );
    for row in rows {
        println!(
            "{}",
            row.iter()
                .zip(widths)
                .map(|(cell, width)| format!("{cell:<width$}"))
                .collect::<Vec<_>>()
                .join("\t")
        );
    }
}

fn resume(session: &Session, paths: &DataPaths) -> Result<(), String> {
    if !session.project_path.is_dir() {
        return Err(format!(
            "Error: Project directory '{}' no longer exists.",
            session.project_path.display()
        ));
    }
    let argv = provider(session.agent).resume_command(session, paths);
    let error = Command::new(&argv[0])
        .args(&argv[1..])
        .current_dir(&session.project_path)
        .exec();
    Err(format!("Error: {}: {error}", argv[0].to_string_lossy()))
}

fn delete(session: &Session, paths: &DataPaths) -> Result<(), String> {
    let confirming = Arc::new(AtomicBool::new(true));
    let interrupted_confirmation = Arc::clone(&confirming);
    let mut signals = signal_hook::iterator::Signals::new([signal_hook::consts::SIGINT])
        .map_err(|error| format!("Error: Cannot handle interruption: {error}"))?;
    std::thread::spawn(move || {
        if signals.forever().next().is_some() {
            if interrupted_confirmation.load(Ordering::SeqCst) {
                println!("\nAborted.");
                std::process::exit(0);
            }
            std::process::exit(130);
        }
    });
    println!("  Agent:       {}", session.agent.key());
    if let Some(name) = &session.name {
        println!("  Name:        {name}");
    }
    println!("  Session ID:  {}", session.session_id);
    println!("  Project:     {}", session.project_path.display());
    println!("  Created:     {}", format_datetime(session.created_at));
    println!("  Last active: {}", format_datetime(session.last_active_at));
    println!(
        "  Last command: {}\n",
        truncate(session.latest_command.trim(), DELETE_COMMAND_MAX)
    );
    print!("Delete this session? [y/N] ");
    io::stdout()
        .flush()
        .map_err(|error| format!("Error: {error}"))?;
    let mut answer = String::new();
    let input = io::stdin().read_line(&mut answer);
    confirming.store(false, Ordering::SeqCst);
    match input {
        Ok(0) => println!("\nAborted."),
        Err(error) if error.kind() == io::ErrorKind::Interrupted => println!("\nAborted."),
        Err(error) => return Err(format!("Error: Cannot read confirmation: {error}")),
        Ok(_) => {
            if !answer.trim().eq_ignore_ascii_case("y") {
                println!("Aborted.");
                return Ok(());
            }
            let errors = provider(session.agent).delete_sessions(&[session], paths, &ProcessRunner);
            if !errors.is_empty() {
                return Err(errors
                    .iter()
                    .map(|error| format!("Error: {error}"))
                    .collect::<Vec<_>>()
                    .join("\n"));
            }
            println!("Session '{}' deleted.", session.session_id);
        }
    }
    Ok(())
}
