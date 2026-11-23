//! Explicit normal-exit state; cleanup must finish before the second exit request.
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Default)]
pub struct Shutdown(AtomicU8);

#[derive(Debug, PartialEq, Eq)]
pub enum ExitAction {
    StartCleanup,
    Wait,
    Exit,
}

impl Shutdown {
    pub fn request(&self) -> ExitAction {
        match self
            .0
            .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
        {
            Ok(_) => ExitAction::StartCleanup,
            Err(2) => ExitAction::Exit,
            Err(_) => ExitAction::Wait,
        }
    }

    pub fn closing(&self) -> bool {
        self.0.load(Ordering::SeqCst) != 0
    }

    pub fn complete(&self) {
        self.0.store(2, Ordering::SeqCst);
    }
}

/// Resources are explicitly stopped while the application state still exists.
pub trait Resources: Send + Sync + 'static {
    fn exit_state(&self) -> &Shutdown;
    fn stop(&self);
}

pub fn on_window_event<R: tauri::Runtime>(window: &tauri::Window<R>, event: &tauri::WindowEvent) {
    use tauri::Manager;
    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        api.prevent_close();
        window.app_handle().exit(0);
    }
}

pub fn on_event<S: Resources, R: tauri::Runtime>(
    handle: &tauri::AppHandle<R>,
    event: tauri::RunEvent,
) {
    use tauri::Manager;
    if let tauri::RunEvent::ExitRequested { api, .. } = event {
        match handle.state::<S>().exit_state().request() {
            ExitAction::Exit => {}
            ExitAction::Wait => api.prevent_exit(),
            ExitAction::StartCleanup => {
                api.prevent_exit();
                if let Some(window) = handle.get_webview_window("main") {
                    let _ = window.set_title("Tauros — Closing…");
                }
                let handle = handle.clone();
                std::thread::spawn(move || {
                    let resources = handle.state::<S>();
                    resources.stop();
                    resources.exit_state().complete();
                    handle.exit(0);
                });
            }
        }
    }
}
