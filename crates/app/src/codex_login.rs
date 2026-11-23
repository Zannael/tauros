//! Runtime-managed ChatGPT browser login. Tauros never reads or copies tokens.
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;
use tauros_agent_bridge::codex::{CodexAdapter, CodexConfig, CodexTools};

const LOGIN_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginStatus {
    #[default]
    Idle,
    /// A status check found no cached sign-in (used by Antigravity).
    SignedOut,
    Pending,
    SignedIn,
    Cancelled,
    Failed,
    Expired,
}

struct NoTools;
impl CodexTools for NoTools {
    fn definitions(&self) -> Vec<Value> {
        Vec::new()
    }
    fn call(&self, _: &str, _: Value, _: &AtomicBool) -> Result<Value, String> {
        Err("tools are unavailable during sign-in".into())
    }
}

struct Pending {
    runtime: CodexAdapter,
    id: String,
    started: Instant,
}

#[derive(Default)]
pub struct CodexLogin {
    closed: bool,
    pending: Option<Pending>,
    status: LoginStatus,
}

impl CodexLogin {
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }

    /// The opener is host-owned, not a webview-supplied URL or command.
    pub fn start(
        &mut self,
        config: CodexConfig,
        project: &Path,
        open_browser: impl FnOnce(&str) -> Result<(), String>,
    ) -> Result<LoginStatus, String> {
        if self.pending() {
            return Err("ChatGPT sign-in is already pending".into());
        }
        self.status = LoginStatus::Failed;
        if self.closed { return Err("Tauros is shutting down.".into()); }
        crate::chat::validate_codex_home(&config.home, project)?;
        let runtime = CodexAdapter::spawn(config, Arc::new(NoTools)).map_err(|e| e.message)?;
        if runtime.chatgpt_signed_in().map_err(|e| e.message)? {
            self.status = LoginStatus::SignedIn;
            return Ok(self.status);
        }
        let (id, url) = runtime.start_browser_login().map_err(|e| e.message)?;
        if !official_auth_url(&url) {
            let _ = runtime.cancel_browser_login(&id);
            return Err("Codex returned an unsupported sign-in destination".into());
        }
        if open_browser(&url).is_err() {
            let _ = runtime.cancel_browser_login(&id);
            // Do not return an opener error that could contain the OAuth URL.
            return Err("Could not open the browser. Retry sign-in or use the dedicated-home CLI login in README".into());
        }
        self.pending = Some(Pending {
            runtime,
            id,
            started: Instant::now(),
        });
        self.status = LoginStatus::Pending;
        Ok(self.status)
    }

    pub fn poll(&mut self) -> Result<LoginStatus, String> {
        let Some(pending) = &self.pending else {
            return Ok(self.status);
        };
        if pending.started.elapsed() >= LOGIN_TIMEOUT {
            let _ = pending.runtime.cancel_browser_login(&pending.id);
            self.pending = None;
            self.status = LoginStatus::Expired;
            return Ok(self.status);
        }
        let result = pending
            .runtime
            .browser_login_result(&pending.id)
            .and_then(|success| match success {
                Some(true) => pending.runtime.chatgpt_signed_in().map(Some),
                other => Ok(other),
            });
        match result {
            Ok(None) => {}
            Ok(Some(success)) => {
                self.status = if success {
                    LoginStatus::SignedIn
                } else {
                    LoginStatus::Failed
                };
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
        if let Some(pending) = self.pending.take() {
            // Dropping the runtime also closes its callback on RPC failure.
            let result = pending.runtime.cancel_browser_login(&pending.id);
            self.status = LoginStatus::Cancelled;
            result.map_err(|e| e.message)?;
        }
        Ok(self.status)
    }
}

fn official_auth_url(raw: &str) -> bool {
    let Ok(url) = tauri::Url::parse(raw) else {
        return false;
    };
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && matches!(url.host_str(), Some("auth.openai.com" | "chatgpt.com"))
}

/// Open only the validated URL returned by the supervised official runtime.
pub fn open_browser(url: &str) -> Result<(), String> {
    if !official_auth_url(url) {
        return Err("unsupported sign-in destination".into());
    }
    #[cfg(target_os = "linux")]
    {
        gtk::gio::AppInfo::launch_default_for_uri(url, None::<&gtk::gio::AppLaunchContext>)
            .map_err(|_| "could not launch the default browser".into())
    }
    #[cfg(not(target_os = "linux"))]
    Err("browser sign-in is currently supported on Linux only".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn browser_destination_is_official_https_without_userinfo() {
        for url in [
            "https://auth.openai.com/oauth/authorize?state=fixture",
            "https://chatgpt.com/auth",
        ] {
            assert!(official_auth_url(url));
        }
        for url in [
            "http://auth.openai.com/",
            "https://auth.openai.com.evil.test/",
            "file:///tmp/test",
            "https://evil.test/",
            "https://user@chatgpt.com/",
            "https://chatgpt.com:444/",
            "javascript:alert(1)",
        ] {
            assert!(!official_auth_url(url));
        }
    }
}
