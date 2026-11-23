//! Official login lifecycle over the real confined transport; no credentials or inference.
#[path = "../../connector-gen3/tests/common/mod.rs"]
mod common;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use tauros_agent_bridge::codex::CodexConfig;
use tauros_app::codex_login::{CodexLogin, LoginStatus};

fn config(home: &Path, mode: &str) -> CodexConfig {
    fs::write(home.join("login-mode"), mode).unwrap();
    let mut config = CodexConfig::new(home);
    config.binary =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex_login_server.py");
    config
}

#[test]
fn browser_login_completion_cancel_and_failure_stay_private() {
    let project = common::TempDir::new("login-project");
    for (mode, expected) in [
        ("success", LoginStatus::SignedIn),
        ("failure", LoginStatus::Failed),
        ("stale", LoginStatus::Pending),
        ("pending", LoginStatus::Pending),
        ("already", LoginStatus::SignedIn),
    ] {
        let home = common::TempDir::new("login-home");
        let mut login = CodexLogin::default();
        let opened = AtomicBool::new(false);
        let status = login
            .start(config(home.path(), mode), project.path(), |url| {
                assert!(url.starts_with("https://auth.openai.com/"));
                opened.store(true, Ordering::SeqCst);
                Ok(())
            })
            .unwrap();
        assert_eq!(opened.load(Ordering::SeqCst), mode != "already");
        assert_eq!(
            status,
            if mode == "already" {
                LoginStatus::SignedIn
            } else {
                LoginStatus::Pending
            }
        );
        assert_eq!(login.poll().unwrap(), expected);
        let serialized = serde_json::to_string(&login.poll().unwrap()).unwrap();
        assert!(!serialized.contains("PRIVATE_"));
        if login.pending() {
            assert!(login
                .start(config(home.path(), mode), project.path(), |_| panic!(
                    "duplicate opener"
                ))
                .is_err());
            assert_eq!(login.cancel().unwrap(), LoginStatus::Cancelled);
            assert!(home.path().join("cancelled").is_file());
            assert_eq!(login.poll().unwrap(), LoginStatus::Cancelled);
            // Retry has a fresh runtime/attempt and no stale success.
            assert_eq!(
                login
                    .start(config(home.path(), "success"), project.path(), |_| Ok(()))
                    .unwrap(),
                LoginStatus::Pending
            );
            assert_eq!(login.poll().unwrap(), LoginStatus::SignedIn);
        }
    }
}

#[test]
fn browser_launch_failure_unsafe_url_and_disconnect_fail_closed() {
    let project = common::TempDir::new("login-project-failure");
    for mode in ["unsafe", "pending"] {
        let home = common::TempDir::new("login-home-failure");
        let mut login = CodexLogin::default();
        let error = login
            .start(config(home.path(), mode), project.path(), |_| {
                assert_ne!(mode, "unsafe", "unsafe URL reached browser");
                Err("PRIVATE_URL_SENTINEL".into())
            })
            .unwrap_err();
        assert!(!error.contains("PRIVATE_"));
        assert!(!login.pending());
        assert_eq!(login.poll().unwrap(), LoginStatus::Failed);
        assert!(home.path().join("cancelled").is_file());
    }
    let home = common::TempDir::new("login-home-eof");
    let mut login = CodexLogin::default();
    login
        .start(config(home.path(), "eof"), project.path(), |_| Ok(()))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if login.poll().is_err() {
            break;
        }
        assert!(Instant::now() < deadline, "EOF did not end login");
        thread::sleep(Duration::from_millis(10));
    }
    assert!(!login.pending());
    assert_eq!(login.poll().unwrap(), LoginStatus::Failed);
    let untouched = project.path().join("never-created");
    assert!(login
        .start(CodexConfig::new(&untouched), project.path(), |_| panic!(
            "overlapping home opened browser"
        ))
        .is_err());
    assert!(
        !untouched.exists(),
        "rejected home changed the project before a snapshot"
    );
    let nested = project.path().join("bad-home");
    fs::create_dir(&nested).unwrap();
    assert!(login
        .start(config(&nested, "success"), project.path(), |_| panic!(
            "project home reached browser"
        ))
        .is_err());
}

#[test]
#[ignore = "requires TAUROS_CODEX_LOGIN_PROTOCOL=1; starts/cancels official browser flow, no sign-in or model call"]
fn installed_codex_browser_login_start_and_cancel_without_signing_in() {
    assert_eq!(
        std::env::var("TAUROS_CODEX_LOGIN_PROTOCOL").as_deref(),
        Ok("1")
    );
    let home = common::TempDir::new("official-login-empty-home");
    let project = common::TempDir::new("official-login-project");
    let mut config = CodexConfig::new(home.path());
    if let Some(binary) = std::env::var_os("TAUROS_CODEX_BIN") {
        config.binary = binary.into();
    }
    let mut login = CodexLogin::default();
    assert_eq!(
        login.start(config, project.path(), |_| Ok(())).unwrap(),
        LoginStatus::Pending
    );
    assert_eq!(login.poll().unwrap(), LoginStatus::Pending);
    assert_eq!(login.cancel().unwrap(), LoginStatus::Cancelled);
    assert!(!home.path().join("auth.json").exists());
}

fn network_probe(online: bool) -> serde_json::Value {
    use std::sync::Arc;
    use tauros_agent_bridge::codex::CodexAdapter;
    use tauros_app::codex_tools::ProjectCodexTools;
    let home = common::TempDir::new("codex-network-home");
    let project = common::TempDir::new("codex-network-project");
    if online { fs::write(home.path().join("online-probe"), "yes").unwrap(); }
    let mut config = CodexConfig::new(home.path());
    config.binary = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex_network_server.py");
    let adapter = CodexAdapter::spawn(config, Arc::new(ProjectCodexTools::new(project.path().into(), None))).unwrap();
    assert!(!adapter.chatgpt_signed_in().unwrap());
    assert!(!home.path().join("auth.json").exists());
    serde_json::from_str(&fs::read_to_string(home.path().join("network-result.json")).unwrap()).unwrap()
}

#[test]
fn system_resolver_is_available_inside_runtime_namespace() {
    assert_eq!(network_probe(false)["resolver_available"], true);
}

#[test]
#[ignore = "requires TAUROS_CODEX_NETWORK_TESTS=1; unauthenticated DNS/HTTPS only, no login or inference"]
fn runtime_can_resolve_and_reach_official_auth_with_tls() {
    assert_eq!(std::env::var("TAUROS_CODEX_NETWORK_TESTS").as_deref(), Ok("1"));
    let report = network_probe(true);
    assert_eq!(report["dns"], true);
    assert_eq!(report["tls_verified"], true);
    assert!((200..600).contains(&report["https_status"].as_u64().unwrap()));
    eprintln!("public OAuth endpoint reachable with verified TLS; HTTP {}", report["https_status"]);
}

#[test]
fn shutdown_cancels_pending_login_and_refuses_another_browser_flow() {
    let project = common::TempDir::new("login-shutdown-project");
    let home = common::TempDir::new("login-shutdown-home");
    let mut login = CodexLogin::default();
    login.start(config(home.path(), "pending"), project.path(), |_| Ok(())).unwrap();
    assert!(login.pending());
    login.shutdown();
    login.shutdown();
    assert!(!login.pending());
    assert!(home.path().join("cancelled").exists());
    assert!(login.start(config(home.path(), "pending"), project.path(), |_| panic!("browser after shutdown"))
        .unwrap_err().contains("shutting down"));
}
