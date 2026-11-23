//! AGT-010: Google sign-in through ACP `authenticate` against an original
//! deterministic peer; no browser, credentials or inference.
#[path = "../../connector-gen3/tests/common/mod.rs"]
mod common;
use std::fs;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};
use tauros_agent_bridge::antigravity::AntigravityConfig;
use tauros_app::antigravity_login::{AntigravityLogin, LoginStatus};

fn config(home: &Path, mode: &str) -> AntigravityConfig {
    fs::create_dir_all(home).unwrap();
    fs::write(home.join("login-mode"), mode).unwrap();
    let mut config = AntigravityConfig::new(home);
    config.binary = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/agy_acp_server.py");
    config
}

fn settle(login: &mut AntigravityLogin, wait: Duration) -> LoginStatus {
    let deadline = Instant::now() + wait;
    loop {
        let status = login.poll().unwrap();
        if status != LoginStatus::Pending || Instant::now() > deadline {
            return status;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn alive(pid: &str) -> bool {
    // A killed child stays a zombie only until the adapter reaps it in Drop.
    fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| !stat.contains(") Z "))
}

#[test]
fn check_start_success_failure_and_cancel_expose_only_status() {
    let project = common::TempDir::new("agy-login-project");
    for (mode, checked, settled) in [
        ("already", LoginStatus::SignedIn, LoginStatus::SignedIn),
        ("success", LoginStatus::SignedOut, LoginStatus::SignedIn),
        ("failure", LoginStatus::SignedOut, LoginStatus::Failed),
        ("pending", LoginStatus::SignedOut, LoginStatus::Pending),
    ] {
        let home = common::TempDir::new("agy-login-home");
        let mut login = AntigravityLogin::default();
        assert_eq!(login.check(config(home.path(), mode), project.path()).unwrap(), checked, "{mode}");
        assert!(!home.path().join("login-started").exists(), "a status check must not start sign-in");
        let started = login.start(config(home.path(), mode), project.path()).unwrap();
        assert_eq!(started, if mode == "already" { LoginStatus::SignedIn } else { LoginStatus::Pending }, "{mode}");
        // `start` returns once the request is sent; the peer records it shortly after.
        let marker = home.path().join("login-started");
        let deadline = Instant::now() + Duration::from_secs(5);
        while mode != "already" && fs::read_to_string(&marker).map_or(true, |pid| pid.is_empty()) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(marker.exists(), mode != "already", "{mode}");
        let wait = Duration::from_secs(if settled == LoginStatus::Pending { 1 } else { 10 });
        assert_eq!(settle(&mut login, wait), settled, "{mode}");
        let serialized = serde_json::to_string(&login.poll().unwrap()).unwrap();
        assert!(!serialized.contains("PRIVATE"), "{serialized}");
        if login.pending() {
            assert!(login.start(config(home.path(), mode), project.path()).is_err(), "duplicate start");
            assert_eq!(login.check(config(home.path(), mode), project.path()).unwrap(), LoginStatus::Pending);
            let pid = fs::read_to_string(home.path().join("login-started")).unwrap();
            assert!(alive(&pid));
            assert_eq!(login.cancel().unwrap(), LoginStatus::Cancelled);
            assert!(!alive(&pid), "cancel must terminate the runtime");
            assert_eq!(login.poll().unwrap(), LoginStatus::Cancelled);
            assert!(!login.pending());
        }
    }
    // A runtime home inside the project is refused before anything starts.
    let mut login = AntigravityLogin::default();
    let error = login.start(config(&project.path().join("agy"), "success"), project.path()).unwrap_err();
    assert!(error.contains("separate directories"), "{error}");
}

/// The installed CLI in a namespace where the user's home and session bus are
/// hidden, so it starts signed out and the Owner's real sign-in is untouched.
/// The browser opener is a stub that records only the destination host.
#[test]
#[ignore = "requires TAUROS_AGY_BIN and /usr/bin/bwrap; starts and cancels a real sign-in without completing it"]
fn installed_antigravity_sign_in_starts_and_cancels_in_isolation() {
    use std::os::unix::fs::PermissionsExt;
    let agy = fs::canonicalize(std::env::var_os("TAUROS_AGY_BIN").expect("TAUROS_AGY_BIN")).unwrap();
    let root = common::TempDir::new("agy-isolated");
    let root = fs::canonicalize(root.path()).unwrap();
    for dir in ["home", "bin"] {
        fs::create_dir_all(root.join(dir)).unwrap();
    }
    let host = root.join("opened-host");
    let opener = format!("#!/bin/sh\necho \"$1\" | sed -E 's#^https?://([^/:]+).*#\\1#' > '{}'\n", host.display());
    let wrapper = format!(
        "#!/bin/sh\nexec /usr/bin/bwrap --die-with-parent --dev-bind / / --tmpfs /run/user/$(id -u) --tmpfs \"$HOME\" \
--ro-bind '{agy_dir}' '{agy_dir}' --bind '{root}' '{root}' --setenv HOME '{root}/home' \
--setenv BROWSER '{root}/bin/xdg-open' --setenv PATH '{root}/bin:/usr/bin:/bin' \
--unsetenv DBUS_SESSION_BUS_ADDRESS --chdir \"$PWD\" '{agy}' \"$@\"\n",
        agy_dir = agy.parent().unwrap().display(), root = root.display(), agy = agy.display());
    for (name, body) in [("bin/xdg-open", &opener), ("agy", &wrapper)] {
        fs::write(root.join(name), body).unwrap();
        fs::set_permissions(root.join(name), fs::Permissions::from_mode(0o700)).unwrap();
    }
    let project = common::TempDir::new("agy-isolated-project");
    let mut config = AntigravityConfig::new(root.join("runtime"));
    config.binary = root.join("agy");
    let mut login = AntigravityLogin::default();
    assert_eq!(login.check(config.clone(), project.path()).unwrap(), LoginStatus::SignedOut);
    assert_eq!(login.start(config, project.path()).unwrap(), LoginStatus::Pending);
    let deadline = Instant::now() + Duration::from_secs(20);
    while !host.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(fs::read_to_string(&host).unwrap().trim(), "accounts.google.com");
    assert_eq!(login.poll().unwrap(), LoginStatus::Pending);
    assert_eq!(login.cancel().unwrap(), LoginStatus::Cancelled);
    assert!(!login.pending());
}

#[test]
fn shutdown_stops_pending_google_login_and_refuses_restart() {
    let project = common::TempDir::new("agy-login-shutdown-project");
    let home = common::TempDir::new("agy-login-shutdown-home");
    let mut login = AntigravityLogin::default();
    login.start(config(home.path(), "pending"), project.path()).unwrap();
    assert!(login.pending());
    login.shutdown();
    login.shutdown();
    assert!(!login.pending());
    assert!(login.start(config(home.path(), "pending"), project.path()).unwrap_err().contains("shutting down"));
    assert!(login.check(config(home.path(), "pending"), project.path()).unwrap_err().contains("shutting down"));
}
