//! Wire the `Bridge` global's callbacks to the backend: each callback
//! captures `Rc<AppServices>` (the runtime must live in `AppServices`, not in
//! the spawned futures), spawns a task, and reports through `UiHandle`.

use std::rc::Rc;

use crate::bridge::UiHandle;
use crate::services::AppServices;
use crate::{Bridge, MainWindow};
use slint::ComponentHandle;

/// Build the window, attach callbacks, load the initial sidebar list, run.
pub fn run(services: AppServices) -> anyhow::Result<()> {
    let window = MainWindow::new()?;
    tracing::debug!(theme = %services.config.appearance.theme, "loaded config");
    let ui = UiHandle::new(&window);
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
        bridge.on_connect_profile(move |id| {
            let backend = services.backend.clone();
            let ui = ui.clone();
            services.runtime.spawn(async move {
                backend.connect_profile(id, ui).await;
            });
        });
    }

    window.run()?;
    Ok(())
}
