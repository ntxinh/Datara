//! Wire the `Bridge` global's callbacks to the backend: each callback
//! captures `Rc<AppServices>` (the runtime must live in `AppServices`, not in
//! the spawned futures), spawns a task, and reports through `UiHandle`.

use std::rc::Rc;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::bridge::{push_tree, UiHandle};
use crate::schema_tree::SchemaTree;
use crate::services::AppServices;
use crate::{Bridge, MainWindow};
use slint::ComponentHandle;

/// Build the window, attach callbacks, load the initial sidebar list, run.
pub fn run(services: AppServices) -> anyhow::Result<()> {
    let window = MainWindow::new()?;
    tracing::debug!(theme = %services.config.appearance.theme, "loaded config");
    let tree = Arc::new(Mutex::new(SchemaTree::default()));
    let ui = UiHandle::new(&window, Arc::clone(&tree));
    let services = Rc::new(services);

    // Initial sidebar population.
    {
        let backend = Rc::clone(&services).backend.clone();
        let ui = ui.clone();
        services.runtime.spawn(async move {
            backend.reload_connections(&ui).await;
        });
    }

    let bridge = window.global::<Bridge>();

    {
        let services = Rc::clone(&services);
        let ui = ui.clone();
        bridge.on_new_connection(move |form| {
            let backend = services.backend.clone();
            let ui = ui.clone();
            services.runtime.spawn(async move {
                backend.save_connection(form, ui).await;
            });
        });
    }

    {
        let services = Rc::clone(&services);
        let ui = ui.clone();
        bridge.on_test_connection(move |form| {
            let backend = services.backend.clone();
            let ui = ui.clone();
            services.runtime.spawn(async move {
                backend.test_form(form, ui).await;
            });
        });
    }

    {
        let services = Rc::clone(&services);
        let ui = ui.clone();
        let tree = Arc::clone(&tree);
        let weak = window.as_weak();
        bridge.on_toggle_node(move |id| {
            let backend = services.backend.clone();
            let ui = ui.clone();
            let node = {
                let Some(win) = weak.upgrade() else {
                    return;
                };
                let bridge = win.global::<Bridge>();
                let mut tree = tree.lock();
                let Some(idx) = tree.find(id) else {
                    return;
                };
                if tree.visible()[idx].expanded {
                    tree.collapse(idx);
                    push_tree(&bridge, &tree);
                    return;
                }
                let Some(node) = tree.expandable(idx).cloned() else {
                    return; // leaf or placeholder: nothing to fetch
                };
                tree.expand_placeholder(idx);
                push_tree(&bridge, &tree);
                node
            };
            services.runtime.spawn(async move {
                backend.expand_node(node, ui).await;
            });
        });
    }

    {
        let ui = ui.clone();
        let limit = services.config.query.default_limit;
        bridge.on_open_table(move |id| {
            if let Some(event) = crate::bridge::open_table_event(&tree.lock(), id, limit) {
                ui.dispatch(event);
            }
        });
    }

    window.run()?;
    Ok(())
}
