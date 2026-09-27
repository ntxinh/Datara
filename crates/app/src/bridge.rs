//! Slint ↔ Tokio boundary. Backend tasks spawn on the Tokio runtime and
//! report back through [`UiHandle::dispatch`], which hops onto the Slint
//! event loop (`invoke_from_event_loop`) before touching the window.

use crate::{Bridge, ConnectionItem, MainWindow};
use datara_domain::{ConnectionId, ConnectionProfile};
use slint::{ComponentHandle, Model, ModelRc, VecModel, Weak};

/// Events flowing core → UI.
pub enum AppEvent {
    /// Full connection list refresh (initial load, after save/delete).
    ConnectionsLoaded(Vec<ConnectionProfile>),
    /// Free-form status line shown at the bottom of the sidebar.
    Status(String),
    /// Dialog Test button finished: `Ok(())` or the error's `Display`.
    ConnectTestResult(Result<(), String>),
    /// `connect-profile` succeeded; a pooled session exists for this id.
    Connected(ConnectionId),
    /// `connect-profile` failed.
    ConnectFailed { id: ConnectionId, message: String },
}

/// Weak handle to the window, safe to move into Tokio tasks.
#[derive(Clone, Default)]
pub struct UiHandle {
    weak: Weak<MainWindow>,
}

impl UiHandle {
    pub fn new(window: &MainWindow) -> Self {
        Self {
            weak: window.as_weak(),
        }
    }

    /// Post `event` onto the Slint event loop; no-op if the window is gone.
    pub fn dispatch(&self, event: AppEvent) {
        let weak = self.weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            let Some(window) = weak.upgrade() else {
                return;
            };
            apply(&window, event);
        });
    }
}

fn apply(window: &MainWindow, event: AppEvent) {
    let bridge = window.global::<Bridge>();
    match event {
        AppEvent::ConnectionsLoaded(profiles) => {
            let items: Vec<ConnectionItem> = profiles
                .into_iter()
                .map(|p| ConnectionItem {
                    id: p.id.0 as i32,
                    name: p.name.into(),
                    host: p.host.into(),
                })
                .collect();
            bridge.set_connections(ModelRc::new(VecModel::from(items)));
        }
        AppEvent::Status(s) => bridge.set_status(s.into()),
        AppEvent::ConnectTestResult(Ok(())) => {
            bridge.set_test_result("Connection OK".into());
        }
        AppEvent::ConnectTestResult(Err(e)) => {
            bridge.set_test_result(format!("Failed: {e}").into());
        }
        AppEvent::Connected(id) => {
            let name = connection_name(&bridge, id);
            bridge.set_status(format!("Connected to {name}").into());
        }
        AppEvent::ConnectFailed { id, message } => {
            let name = connection_name(&bridge, id);
            bridge.set_status(format!("Connect to {name} failed: {message}").into());
        }
    }
}

/// Look up a profile's display name in the current sidebar model.
fn connection_name(bridge: &Bridge, id: ConnectionId) -> String {
    bridge
        .get_connections()
        .iter()
        .find(|c| i64::from(c.id) == id.0)
        .map(|c| c.name.to_string())
        .unwrap_or_else(|| format!("#{}", id.0))
}
