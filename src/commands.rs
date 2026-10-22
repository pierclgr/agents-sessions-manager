//! the command-line contract shared by both installed command names

use crate::{
    data::{
        DataPaths, Session,
        agents::{load_session_names, load_session_summaries, provider},
    },
    tui,
    utils::{ProcessRunner, SHORT_ID_LENGTH, format_datetime, truncate},
};
use std::{
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
const LIST_USAGE: &str = "Usage: agents-sessions ls [-a | --all]";

const HELP: &str = "Usage: agents-sessions [COMMAND] [OPTIONS]

Agents Sessions Manager - Browse, resume, and delete coding agent sessions.
Supported agents: Claude Code, Codex, OpenCode, Pi.

Commands:
  (none)              Launch the interactive session browser
  ls                  List sessions from the current directory
                      Use -a or --all to list every directory
  launch <ID>         Resume a session by its ID (any agent)
  delete <ID>         Delete a session and all its data (any agent)

Options:
  -a, --all           With 'ls', include sessions from every directory
  -h, --help          Show this help message and exit

Examples:
  agents-sessions                         # Open interactive browser
  agents-sessions ls                      # List sessions from the current directory
  agents-sessions ls --all                # List sessions from all directories
  agents-sessions launch abc123           # Resume session abc123
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
    if matches!(command.as_str(), "launch" | "delete") && args.get(1).is_none_or(|id| id.is_empty())
    {
        return Err(format!(
            "Error: '{command}' requires a session ID argument.\nUsage: agents-sessions {command} <SESSION_ID>"
        ));
    }
    if !matches!(command.as_str(), "" | "ls" | "launch" | "delete") {
        return Err(format!(
            "Error: Unknown command '{command}'.\nRun 'agents-sessions --help' for usage information."
        ));
    }
    let list_all = if command == "ls" {
        let all = match args.get(1) {
            None => false,
            Some(option) if option == "-a" || option == "--all" => true,
            Some(option) => {
                return Err(format!(
                    "Error: Unknown option '{}' for 'ls'.\n{LIST_USAGE}",
                    option.to_string_lossy()
                ));
            }
        };
        if args.len() > 2 {
            return Err(format!(
                "Error: 'ls' accepts only one option.\n{LIST_USAGE}"
            ));
        }
        all
    } else {
        false
    };
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
            list(&sessions);
        }
        _ => {
            let id = args[1].to_string_lossy();
            let mut sessions = load_session_summaries(&paths, Some(id.as_ref()))?;
            if sessions.is_empty() {
                return Err(format!("Error: Session '{id}' not found."));
            }
            if command == "launch" {
                resume(&sessions[0], &paths)?;
            } else {
                load_session_names(&mut sessions, &[0], &paths);
                delete(&sessions[0], &paths)?;
            }
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
