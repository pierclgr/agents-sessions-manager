//! shared session model and data-directory resolution

pub mod agents;
pub mod delete;

use chrono::{DateTime, Utc};
use std::{ffi::OsString, path::PathBuf};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Agent {
    Claude,
    Codex,
    OpenCode,
    Pi,
}

impl Agent {
    pub const ALL: [Self; 4] = [Self::Claude, Self::Codex, Self::OpenCode, Self::Pi];

    pub fn key(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
            Self::Pi => "pi",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
            Self::OpenCode => "OpenCode",
            Self::Pi => "Pi",
        }
    }
}

pub type SessionKey = (Agent, String);

#[derive(Clone, Debug)]
pub struct Session {
    pub agent: Agent,
    pub session_id: String,
    pub project_path: PathBuf,
    pub created_at: DateTime<Utc>,
    pub last_active_at: DateTime<Utc>,
    pub latest_command: String,
    pub commands: Vec<String>,
    pub name: Option<String>,
    pub todos: Vec<String>,
}

impl Session {
    pub fn key(&self) -> SessionKey {
        (self.agent, self.session_id.clone())
    }
}

#[derive(Clone, Debug)]
pub struct DataPaths {
    pub claude: PathBuf,
    pub codex: PathBuf,
    pub opencode_db: PathBuf,
    pub pi_sessions: PathBuf,
}

impl DataPaths {
    pub fn from_env() -> Result<Self, String> {
        let home = dirs::home_dir().ok_or("Error: Cannot find the home directory.")?;
        Ok(Self::from_lookup(home, |key| std::env::var_os(key)))
    }

    /// resolve overrides without modifying process-wide environment variables
    pub fn from_lookup(home: PathBuf, lookup: impl Fn(&str) -> Option<OsString>) -> Self {
        let get = |key| {
            lookup(key)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        Self {
            claude: get("CLAUDE_DATA_DIR").unwrap_or_else(|| home.join(".claude")),
            codex: get("CODEX_HOME").unwrap_or_else(|| home.join(".codex")),
            opencode_db: get("XDG_DATA_HOME")
                .unwrap_or_else(|| home.join(".local/share"))
                .join("opencode/opencode.db"),
            pi_sessions: get("PI_CODING_AGENT_SESSION_DIR").unwrap_or_else(|| {
                get("PI_CODING_AGENT_DIR")
                    .unwrap_or_else(|| home.join(".pi/agent"))
                    .join("sessions")
            }),
        }
    }
}
