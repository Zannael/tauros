//! Normal-exit state and a real Tauri close while another owner retains Chat.
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use tauros_agent_bridge::fake::{FakeAdapter, FakeStep, FakeTurnScript};
use tauros_agent_bridge::{AgentBridge, SessionConfig};
use tauros_app::chat::{Chat, ChatBackend};
use tauros_app::shutdown::{ExitAction, Resources, Shutdown};

#[test]
fn repeated_exit_waits_for_cleanup_and_only_then_allows_exit() {
    let state = Shutdown::default();
    assert!(!state.closing());
    assert_eq!(state.request(), ExitAction::StartCleanup);
    assert!(state.closing());
    assert_eq!(state.request(), ExitAction::Wait);
    state.complete();
    assert_eq!(state.request(), ExitAction::Exit);
}

struct Probe {
    state: Shutdown,
    chat: Arc<Chat>,
    cleaned: Arc<AtomicUsize>,
    started: mpsc::Sender<()>,
    release: Mutex<mpsc::Receiver<()>>,
}
impl Resources for Probe {
    fn exit_state(&self) -> &Shutdown {
        &self.state
    }
    fn stop(&self) {
        self.chat.shutdown();
        self.cleaned.fetch_add(1, Ordering::SeqCst);
        self.started.send(()).unwrap();
        // Hold cleanup open while the test requests exit again.
        self.release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
    }
}

#[test]
#[ignore = "requires an isolated graphical display; run under xvfb-run"]
fn native_window_close_waits_for_active_chat_cleanup_even_with_repeated_exit() {
    use tauri::Manager;
    let chat = Arc::new(Chat::new(|| {
        let mut bridge = FakeAdapter::new();
        bridge.push_script(FakeTurnScript {
            steps: vec![FakeStep::WaitForGate],
            usage: None,
        });
        let session = bridge
            .start_session(SessionConfig::default())
            .map_err(|e| e.message)?;
        Ok(ChatBackend {
            bridge: Box::new(bridge),
            session,
            keep_alive: Box::new(()),
        })
    }));
    let (events_tx, events_rx) = mpsc::channel();
    chat.send("wait", move |event| {
        let _ = events_tx.send(event);
    })
    .unwrap();
    events_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let retained_by_emulator = chat.clone();
    let cleaned = Arc::new(AtomicUsize::new(0));
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let probe = Probe {
        state: Shutdown::default(),
        chat,
        cleaned: cleaned.clone(),
        started: started_tx,
        release: Mutex::new(release_rx),
    };
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows[0].visible = false;
    context.config_mut().app.windows[0].url =
        tauri::WebviewUrl::External("about:blank".parse().unwrap());
    let driver = Arc::new(Mutex::new(None));
    let driver_slot = driver.clone();
    let app = tauri::Builder::default()
        .any_thread()
        .manage(probe)
        .on_window_event(tauros_app::shutdown::on_window_event)
        .setup(move |app| {
            let handle = app.handle().clone();
            *driver_slot.lock().unwrap() = Some(std::thread::spawn(move || {
                handle.get_webview_window("main").unwrap().close().unwrap();
                started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                handle.exit(0);
                std::thread::sleep(Duration::from_millis(150));
                assert!(handle.state::<Probe>().exit_state().closing());
                release_tx.send(()).unwrap();
            }));
            Ok(())
        })
        .build(context)
        .expect("isolated native app");
    let exit = app.run_return(tauros_app::shutdown::on_event::<Probe, _>);
    driver.lock().unwrap().take().unwrap().join().unwrap();
    assert_eq!(exit, 0);
    assert_eq!(cleaned.load(Ordering::SeqCst), 1);
    assert!(!retained_by_emulator.busy());
    assert!(retained_by_emulator
        .send("must not restart", |_| {})
        .unwrap_err()
        .contains("shutting down"));
}
