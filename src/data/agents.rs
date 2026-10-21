//! provider registry in the order used by filter keys 1–4

mod claude;
mod codex;
mod opencode;
mod pi;

pub(crate) use claude::TranscriptIndex;

use super::{Agent, DataPaths, RECENT_COMMAND_LIMIT, Session};
use crate::utils::CommandRunner;
use std::{cmp::Reverse, ffi::OsString};

pub trait AgentProvider: Sync {
    fn load_sessions(&self, paths: &DataPaths) -> Result<Vec<Session>, String>;
    /// load recent prompts, optionally restricting sessions to a literal ID prefix
    fn load_summaries(
        &self,
        paths: &DataPaths,
        id_prefix: Option<&str>,
    ) -> Result<Vec<Session>, String> {
        let mut sessions = self.load_sessions(paths)?;
        sessions.retain(|session| {
            id_prefix.is_none_or(|prefix| session.session_id.starts_with(prefix))
        });
        for session in &mut sessions {
            let count = session.commands.len().saturating_sub(RECENT_COMMAND_LIMIT);
            session.commands.drain(..count);
        }
        Ok(sessions)
    }
    fn load_names(&self, _sessions: &mut [Session], _indices: &[usize], _paths: &DataPaths) {}
    fn load_detail(&self, _session: &mut Session, _paths: &DataPaths) {}
    fn delete_sessions(
        &self,
        sessions: &[&Session],
        paths: &DataPaths,
        runner: &dyn CommandRunner,
    ) -> Vec<String>;
    fn resume_command(&self, session: &Session, paths: &DataPaths) -> Vec<OsString>;
}

#[derive(Clone, Copy)]
pub(crate) enum SessionLoad<'a> {
    Full,
    Recent(Option<&'a str>),
}

impl<'a> SessionLoad<'a> {
    pub(crate) fn id_prefix(self) -> Option<&'a str> {
        match self {
            Self::Full => None,
            Self::Recent(prefix) => prefix,
        }
    }

    pub(crate) fn matches_id(self, id: &str) -> bool {
        self.id_prefix().is_none_or(|prefix| id.starts_with(prefix))
    }

    pub(crate) fn is_full(self) -> bool {
        matches!(self, Self::Full)
    }

    pub(crate) fn push_command(self, commands: &mut Vec<String>, text: String) {
        if !self.is_full() && commands.len() == RECENT_COMMAND_LIMIT {
            commands.remove(0);
        }
        commands.push(text);
    }
}

pub fn provider(agent: Agent) -> &'static dyn AgentProvider {
    match agent {
        Agent::Claude => &claude::Claude,
        Agent::Codex => &codex::Codex,
        Agent::OpenCode => &opencode::OpenCode,
        Agent::Pi => &pi::Pi,
    }
}

pub fn load_all_sessions(paths: &DataPaths) -> Result<Vec<Session>, String> {
    collect_sessions(paths, SessionLoad::Full)
}

/// load only recent prompts and defer transcript names until they are displayed
pub fn load_session_summaries(
    paths: &DataPaths,
    id_prefix: Option<&str>,
) -> Result<Vec<Session>, String> {
    collect_sessions(paths, SessionLoad::Recent(id_prefix))
}

pub fn load_session_names(sessions: &mut [Session], indices: &[usize], paths: &DataPaths) {
    for agent in Agent::ALL {
        let indices: Vec<_> = indices
            .iter()
            .copied()
            .filter(|index| sessions[*index].agent == agent)
            .collect();
        if !indices.is_empty() {
            provider(agent).load_names(sessions, &indices, paths);
        }
    }
}

fn collect_sessions(paths: &DataPaths, load: SessionLoad<'_>) -> Result<Vec<Session>, String> {
    let mut sessions = Vec::new();
    for agent in Agent::ALL {
        sessions.extend(match load {
            SessionLoad::Full => provider(agent).load_sessions(paths)?,
            SessionLoad::Recent(prefix) => provider(agent).load_summaries(paths, prefix)?,
        });
    }
    sessions.sort_by_key(|session| Reverse(session.last_active_at));
    Ok(sessions)
}
