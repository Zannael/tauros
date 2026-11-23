//! Google sign-in for Antigravity through ACP `authenticate` (AGT-010). The
//! runtime opens the browser, hosts the callback and stores credentials; only a
//! status enum leaves this module. The sign-in is shared with the user's other
//! Antigravity surfaces, so Tauros offers no sign-out.
use std::path::Path;
use std::time::{Duration, Instant};

use tauros_agent_bridge::antigravity::{AntigravityAdapter, AntigravityConfig};

pub use crate::codex_login::LoginStatus;

const LOGIN_TIMEOUT: Duration = Duration::from_secs(600);

struct Pending {
    runtime: AntigravityAdapter,
    started: Instant,
}

#[derive(Default)]
pub struct AntigravityLogin {
    closed: bool,
    pending: Option<Pending>,
    status: LoginStatus,
}

impl AntigravityLogin {
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Silent check of the cached sign-in; starts nothing in the browser.
    pub fn check(&mut self, config: AntigravityConfig, project: &Path) -> Result<LoginStatus, String> {
        if self.pending() {
            return Ok(LoginStatus::Pending);
        }
        if self.closed { return Err("Tauros is shutting down.".into()); }
        crate::chat::validate_codex_home(&config.home, project)?;
        let runtime = AntigravityAdapter::spawn(config).map_err(|e| e.message)?;
        self.status = if runtime.signed_in().map_err(|e| e.message)? { LoginStatus::SignedIn } else { LoginStatus::SignedOut };
        Ok(self.status)
    }

    pub fn start(&mut self, config: AntigravityConfig, project: &Path) -> Result<LoginStatus, String> {
        if self.pending() {
            return Err("Google sign-in is already pending".into());
        }
        self.status = LoginStatus::Failed;
        if self.closed { return Err("Tauros is shutting down.".into()); }
        crate::chat::validate_codex_home(&config.home, project)?;
        let runtime = AntigravityAdapter::spawn(config).map_err(|e| e.message)?;
        if runtime.signed_in().map_err(|e| e.message)? {
            self.status = LoginStatus::SignedIn;
            return Ok(self.status);
        }
        runtime.start_google_login().map_err(|e| e.message)?;
        self.pending = Some(Pending { runtime, started: Instant::now() });
        self.status = LoginStatus::Pending;
        Ok(self.status)
    }

    pub fn poll(&mut self) -> Result<LoginStatus, String> {
        let Some(pending) = &self.pending else {
            return Ok(self.status);
        };
        if pending.started.elapsed() >= LOGIN_TIMEOUT {
            // Dropping the runtime terminates it and its callback listener.
            self.pending = None;
            self.status = LoginStatus::Expired;
            return Ok(self.status);
        }
        let result = pending.runtime.google_login_result().and_then(|done| match done {
            Some(true) => pending.runtime.signed_in().map(Some),
            other => Ok(other),
        });
        match result {
            Ok(None) => {}
            Ok(Some(success)) => {
                self.status = if success { LoginStatus::SignedIn } else { LoginStatus::Failed };
                self.pending = None;
            }
            Err(error) => {
                self.status = LoginStatus::Failed;
                self.pending = None;
                return Err(error.message);
            }
        }
        Ok(self.status)
    }

    pub fn shutdown(&mut self) {
        self.closed = true;
        let _ = self.cancel();
    }

    pub fn cancel(&mut self) -> Result<LoginStatus, String> {
        if self.pending.take().is_some() {
            self.status = LoginStatus::Cancelled;
        }
        Ok(self.status)
    }
}
