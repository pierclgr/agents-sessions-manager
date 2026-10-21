//! provider registry in the order used by filter keys 1–4

mod claude;
mod codex;
mod opencode;
mod pi;

pub(crate) use claude::TranscriptIndex;

use super::{Agent, DataPaths, Session};
use crate::utils::CommandRunner;
use std::{cmp::Reverse, ffi::OsString};

pub trait AgentProvider: Sync {
    fn load_sessions(&self, paths: &DataPaths) -> Result<Vec<Session>, String>;
    fn load_detail(&self, _session: &mut Session, _paths: &DataPaths) {}
    fn delete_sessions(
        &self,
        sessions: &[&Session],
        paths: &DataPaths,
        runner: &dyn CommandRunner,
    ) -> Vec<String>;
    fn resume_command(&self, session: &Session, paths: &DataPaths) -> Vec<OsString>;
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
    let mut sessions = Vec::new();
    for agent in Agent::ALL {
        sessions.extend(provider(agent).load_sessions(paths)?);
    }
    sessions.sort_by_key(|session| Reverse(session.last_active_at));
    Ok(sessions)
}
