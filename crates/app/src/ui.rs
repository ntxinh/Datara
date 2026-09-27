//! Wire the `Bridge` global's callbacks to the backend: each callback
//! captures `Rc<AppServices>` (the runtime must live in `AppServices`, not in
//! the spawned futures), spawns a task, and reports through `UiHandle`.
//!
//! Commands arrive via `Bridge.command` → [`commands::command_for`] → an
//! unbounded mpsc → a Tokio forwarder → [`AppEvent::Command`] on the UI
//! thread, so the only place a key becomes an action is the commands table.

use std::rc::Rc;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::bridge::{
    catalog_labels, palette_model, push_tabs, push_tree, run_command, spans_model, AppEvent, UiCtx,
    UiHandle,
};
use crate::commands;
use crate::editor_ui::{line_col, line_count, EditorState};
use crate::schema_tree::SchemaTree;
use crate::services::AppServices;
use crate::{Bridge, MainWindow};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

/// Build the window, attach callbacks, load the initial sidebar list, run.
pub fn run(services: AppServices) -> anyhow::Result<()> {
    let window = MainWindow::new()?;
    tracing::debug!(theme = %services.config.appearance.theme, "loaded config");
    let cx = Arc::new(UiCtx {
        tree: Mutex::new(SchemaTree::default()),
        grid: Mutex::new(crate::grid::GridState::default()),
        editor: Mutex::new(EditorState::default()),
        backend: Arc::clone(&services.backend),
        handle: services.runtime.handle().clone(),
        query_limit: services.config.query.default_limit,
        clipboard: Mutex::new(None),
    });
    let ui = UiHandle::new(&window, Arc::clone(&cx));
    let services = Rc::new(services);

    // Initial bridge state: font size + first tab.
    {
        let bridge = window.global::<Bridge>();
        bridge.set_editor_font_size(services.config.editor.font_size.into());
        bridge.set_cursor_position("Ln 1, Col 1".into());
        push_tabs(&bridge, &cx.editor.lock());
    }

    // Initial sidebar population.
    {
        let backend = Rc::clone(&services).backend.clone();
        let ui = ui.clone();
        services.runtime.spawn(async move {
            backend.reload_connections(&ui).await;
        });
    }

    // Command channel: Bridge.command → mpsc → AppEvent::Command.
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    {
        let ui = ui.clone();
        services.runtime.spawn(async move {
            while let Some(cmd) = cmd_rx.recv().await {
                ui.dispatch(AppEvent::Command(cmd));
            }
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
        let cx = Arc::clone(&cx);
        let weak = window.as_weak();
        bridge.on_toggle_node(move |id| {
            let Some(win) = weak.upgrade() else {
                return;
            };
            {
                let mut tree = cx.tree.lock();
                let Some(idx) = tree.find(id) else {
                    return;
                };
                if tree.visible()[idx].expanded {
                    tree.collapse(idx);
                    push_tree(&win.global::<Bridge>(), &tree);
                    return;
                }
            }
            expand_tree_node(&win, &cx, id);
        });
    }

    {
        let ui = ui.clone();
        let cx = Arc::clone(&cx);
        let limit = services.config.query.default_limit;
        bridge.on_open_table(move |id| {
            if let Some(event) = crate::bridge::open_table_event(&cx.tree.lock(), id, limit) {
                ui.dispatch(event);
            }
        });
    }

    // ── Editor ────────────────────────────────────────────────────────

    {
        let cx = Arc::clone(&cx);
        let weak = window.as_weak();
        bridge.on_editor_changed(move |text, cursor| {
            let Some(win) = weak.upgrade() else {
                return;
            };
            let bridge = win.global::<Bridge>();
            let mut editor = cx.editor.lock();
            let caret = cursor.max(0) as usize;
            editor.set_caret(caret, caret);
            editor.stash(text.to_string(), caret);
            bridge.set_line_count(line_count(&text) as i32);
            bridge.set_highlight_spans(spans_model(&text));
            let catalog = catalog_labels(&cx.tree.lock());
            editor.update_completions(&text, caret, &catalog);
            bridge.set_completions(ModelRc::new(VecModel::from(
                editor
                    .completions
                    .iter()
                    .map(|c| SharedString::from(c.as_str()))
                    .collect::<Vec<_>>(),
            )));
            bridge.set_completion_index(editor.completion_index as i32);
        });
    }

    {
        let cx = Arc::clone(&cx);
        let weak = window.as_weak();
        bridge.on_cursor_changed(move |cursor, anchor, _line, _col| {
            let Some(win) = weak.upgrade() else {
                return;
            };
            // Ln/Col come from the byte offset, not pixel math — exact even
            // if the resolved font isn't perfectly monospace.
            let (line, col) = line_col(
                &win.global::<Bridge>().get_editor_text(),
                cursor.max(0) as usize,
            );
            win.global::<Bridge>()
                .set_cursor_position(format!("Ln {line}, Col {col}").into());
            cx.editor
                .lock()
                .set_caret(cursor.max(0) as usize, anchor.max(0) as usize);
        });
    }

    {
        let cmd_tx = cmd_tx.clone();
        bridge.on_command(move |text, ctrl, shift, alt| {
            if let Some(cmd) =
                commands::parse_command(&commands::key_string(&text, ctrl, shift, alt))
            {
                let _ = cmd_tx.send(cmd);
                return true;
            }
            false
        });
    }

    // ── Schema filter + command palette (Task 4.5) ────────────────────

    {
        let cx = Arc::clone(&cx);
        let weak = window.as_weak();
        bridge.on_filter_tree(move |text| {
            let Some(win) = weak.upgrade() else {
                return;
            };
            let mut tree = cx.tree.lock();
            tree.filter(&text);
            push_tree(&win.global::<Bridge>(), &tree);
        });
    }

    {
        let cx = Arc::clone(&cx);
        let weak = window.as_weak();
        bridge.on_palette_edited(move |query| {
            let Some(win) = weak.upgrade() else {
                return;
            };
            let bridge = win.global::<Bridge>();
            bridge.set_palette_items(palette_model(&cx.tree.lock(), &query));
            bridge.set_palette_index(0);
        });
    }

    {
        let cx = Arc::clone(&cx);
        let weak = window.as_weak();
        bridge.on_palette_submit(move || {
            let Some(win) = weak.upgrade() else {
                return;
            };
            submit_palette(&win, &cx);
        });
    }

    {
        let weak = window.as_weak();
        bridge.on_palette_close(move || {
            if let Some(win) = weak.upgrade() {
                win.global::<Bridge>().set_palette_visible(false);
            }
        });
    }

    // ── Execution ────────────────────────────────────────────────────

    {
        let services = Rc::clone(&services);
        let ui = ui.clone();
        let cx = Arc::clone(&cx);
        bridge.on_cancel_query(move || {
            let tab = cx.editor.lock().active_tab().id;
            let backend = Arc::clone(&services.backend);
            let ui = ui.clone();
            services.runtime.spawn(async move {
                backend.cancel_query(tab, ui).await;
            });
        });
    }

    {
        let cx = Arc::clone(&cx);
        let weak = window.as_weak();
        bridge.on_completion_action(move |action| {
            let Some(win) = weak.upgrade() else {
                return;
            };
            let bridge = win.global::<Bridge>();
            let mut editor = cx.editor.lock();
            match action.as_str() {
                "up" => editor.move_completion(-1),
                "down" => editor.move_completion(1),
                "escape" => editor.completions.clear(),
                "accept" => {
                    // The popup's own TouchArea wrote Bridge.completion-index;
                    // it wins over the state's row.
                    editor.completion_index =
                        bridge.get_completion_index().clamp(0, i32::MAX) as usize;
                    let text = bridge.get_editor_text().to_string();
                    if let Some((new_text, cursor)) = editor.apply_completion(&text) {
                        editor.set_caret(cursor, cursor);
                        let jump = editor.cursor_jump(cursor);
                        bridge.set_editor_text(new_text.clone().into());
                        bridge.set_line_count(line_count(&new_text) as i32);
                        bridge.set_highlight_spans(spans_model(&new_text));
                        bridge.set_set_cursor(jump);
                        editor.stash(new_text, cursor);
                    }
                }
                _ => {}
            }
            bridge.set_completions(ModelRc::new(VecModel::from(
                editor
                    .completions
                    .iter()
                    .map(|c| SharedString::from(c.as_str()))
                    .collect::<Vec<_>>(),
            )));
            bridge.set_completion_index(editor.completion_index as i32);
        });
    }

    // ── Tabs ──────────────────────────────────────────────────────────

    {
        let cx = Arc::clone(&cx);
        let weak = window.as_weak();
        bridge.on_new_tab(move || {
            let Some(win) = weak.upgrade() else {
                return;
            };
            let bridge = win.global::<Bridge>();
            let mut editor = cx.editor.lock();
            let caret = editor.cursor;
            editor.stash(bridge.get_editor_text().to_string(), caret);
            editor.new_tab();
            push_tabs(&bridge, &editor);
            let jump = editor.cursor_jump(0);
            bridge.set_editor_text("".into());
            bridge.set_line_count(1);
            bridge.set_highlight_spans(ModelRc::new(VecModel::from(
                Vec::<crate::HighlightSpan>::new(),
            )));
            bridge.set_set_cursor(jump);
        });
    }

    {
        let cx = Arc::clone(&cx);
        let weak = window.as_weak();
        bridge.on_close_tab(move |id| {
            let Some(win) = weak.upgrade() else {
                return;
            };
            let bridge = win.global::<Bridge>();
            let mut editor = cx.editor.lock();
            if let Some((text, cursor)) = editor.close(id, bridge.get_editor_text().to_string()) {
                push_tabs(&bridge, &editor);
                let jump = editor.cursor_jump(cursor);
                bridge.set_editor_text(text.clone().into());
                bridge.set_line_count(line_count(&text) as i32);
                bridge.set_highlight_spans(spans_model(&text));
                bridge.set_set_cursor(jump);
            }
        });
    }

    {
        let cx = Arc::clone(&cx);
        let weak = window.as_weak();
        bridge.on_switch_tab(move |id| {
            let Some(win) = weak.upgrade() else {
                return;
            };
            let bridge = win.global::<Bridge>();
            let mut editor = cx.editor.lock();
            if let Some((text, cursor)) = editor.switch(id, bridge.get_editor_text().to_string()) {
                push_tabs(&bridge, &editor);
                let jump = editor.cursor_jump(cursor);
                bridge.set_editor_text(text.clone().into());
                bridge.set_line_count(line_count(&text) as i32);
                bridge.set_highlight_spans(spans_model(&text));
                bridge.set_set_cursor(jump);
            }
        });
    }

    // ── Result grid (Task 5.2) ───────────────────────────────────────

    {
        let cx = Arc::clone(&cx);
        let weak = window.as_weak();
        bridge.on_grid_select(move |row, col| {
            let Some(win) = weak.upgrade() else { return };
            cx.grid.lock().select(&win.global::<Bridge>(), row, col);
        });
    }

    {
        let cx = Arc::clone(&cx);
        let weak = window.as_weak();
        bridge.on_grid_drag(move |row, x| {
            let Some(win) = weak.upgrade() else { return };
            cx.grid.lock().drag(&win.global::<Bridge>(), row, x);
        });
    }

    {
        let cx = Arc::clone(&cx);
        let weak = window.as_weak();
        bridge.on_copy_selection(move || {
            let Some(win) = weak.upgrade() else { return };
            cx.grid
                .lock()
                .copy(&mut cx.clipboard.lock(), &win.global::<Bridge>());
        });
    }

    {
        let cx = Arc::clone(&cx);
        let weak = window.as_weak();
        bridge.on_resize_column(move |idx, delta| {
            let Some(win) = weak.upgrade() else { return };
            cx.grid.lock().resize(&win.global::<Bridge>(), idx, delta);
        });
    }

    {
        let cx = Arc::clone(&cx);
        let weak = window.as_weak();
        bridge.on_sort_column(move |idx| {
            let Some(win) = weak.upgrade() else { return };
            cx.grid.lock().sort(&win.global::<Bridge>(), idx);
        });
    }

    // ── Query history (Task 6.1) ─────────────────────────────────────

    {
        let cx = Arc::clone(&cx);
        let weak = window.as_weak();
        bridge.on_history_search(move |filter| {
            let Some(win) = weak.upgrade() else { return };
            let conn_id = cx.editor.lock().active_tab().conn_id;
            let backend = Arc::clone(&cx.backend);
            let ui = UiHandle::new(&win, Arc::clone(&cx));
            cx.handle.spawn(async move {
                backend.history_search(conn_id, &filter, ui).await;
            });
        });
    }

    {
        let cx = Arc::clone(&cx);
        let weak = window.as_weak();
        bridge.on_history_rerun(move |id| {
            let Some(win) = weak.upgrade() else { return };
            let backend = Arc::clone(&cx.backend);
            let ui = UiHandle::new(&win, Arc::clone(&cx));
            cx.handle.spawn(async move {
                backend.history_rerun(i64::from(id), ui).await;
            });
        });
    }

    {
        let cx = Arc::clone(&cx);
        let weak = window.as_weak();
        bridge.on_history_copy(move |id| {
            let Some(win) = weak.upgrade() else { return };
            let backend = Arc::clone(&cx.backend);
            let ui = UiHandle::new(&win, Arc::clone(&cx));
            cx.handle.spawn(async move {
                backend.history_copy(i64::from(id), ui).await;
            });
        });
    }

    {
        let cx = Arc::clone(&cx);
        let weak = window.as_weak();
        bridge.on_history_delete(move |id| {
            let Some(win) = weak.upgrade() else { return };
            let conn_id = cx.editor.lock().active_tab().conn_id;
            let filter = win.global::<Bridge>().get_history_query().to_string();
            let backend = Arc::clone(&cx.backend);
            let ui = UiHandle::new(&win, Arc::clone(&cx));
            cx.handle.spawn(async move {
                backend
                    .history_delete(i64::from(id), conn_id, &filter, ui)
                    .await;
            });
        });
    }

    {
        let cx = Arc::clone(&cx);
        let weak = window.as_weak();
        bridge.on_history_toggle(move || {
            let Some(win) = weak.upgrade() else { return };
            crate::bridge::toggle_history(&win, &cx);
        });
    }
    window.run()?;
    Ok(())
}

/// Expand the tree row `id`: mark it expanded, splice the Loading row, and
/// spawn the backend fetch. Shared by the sidebar's toggle and the
/// palette's "Open connection: …" rows.
fn expand_tree_node(win: &MainWindow, cx: &Arc<UiCtx>, id: i32) {
    let node = {
        let mut tree = cx.tree.lock();
        let Some(idx) = tree.find(id) else {
            return;
        };
        let Some(node) = tree.expandable(idx).cloned() else {
            // Already expanded, leaf, or placeholder — nothing to fetch.
            return;
        };
        tree.expand_placeholder(idx);
        push_tree(&win.global::<Bridge>(), &tree);
        node
    };
    let backend = Arc::clone(&cx.backend);
    let ui = UiHandle::new(win, Arc::clone(cx));
    cx.handle.spawn(async move {
        backend.expand_node(node, ui).await;
    });
}

/// Run the selected palette row, then close the palette. Tags: `connect`
/// opens the connection dialog, `open-connection:<id>` expands that
/// connection's tree row, anything else resolves via
/// [`commands::command_by_tag`] into the normal `Command` path.
fn submit_palette(win: &MainWindow, cx: &Arc<UiCtx>) {
    let bridge = win.global::<Bridge>();
    let items = bridge.get_palette_items();
    let idx = bridge.get_palette_index().clamp(0, i32::MAX) as usize;
    let tag = items.row_data(idx).map(|item| item.command.to_string());
    bridge.set_palette_visible(false);
    let Some(tag) = tag else {
        return;
    };
    if tag == "connect" {
        bridge.set_conn_dialog_visible(true);
    } else if let Some(id) = tag
        .strip_prefix("open-connection:")
        .and_then(|s| s.parse::<i32>().ok())
    {
        expand_tree_node(win, cx, id);
    } else if let Some(cmd) = commands::command_by_tag(&tag) {
        run_command(win, cx, cmd);
    }
}

#[cfg(test)]
mod tests {
    //! Headless UI smoke: `Window::dispatch_event` produces the same
    //! `WindowEvent` stream a real backend does, run through the compiled
    //! `.slint` tree on the testing backend with the software rasterizer.
    //! Covers the brief's open questions: Ctrl+Return reaches
    //! `Bridge.command` instead of inserting a newline, and the highlight
    //! overlay actually lands colored pixels in the editor area.

    use super::*;
    use datara_domain::Command;
    use slint::platform::{Key, WindowEvent};
    use std::cell::RefCell;

    fn press(win: &slint::Window, text: impl Into<slint::SharedString>) {
        let text = text.into();
        win.dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
        win.dispatch_event(WindowEvent::KeyReleased { text });
    }

    /// Modifier press + key press + key release + modifier release — the
    /// same sequence `ui_smoke_keys_and_overlay` uses for Ctrl+Return.
    fn press_ctrl(win: &slint::Window, key: impl Into<slint::SharedString>) {
        let key = key.into();
        win.dispatch_event(WindowEvent::KeyPressed {
            text: Key::Control.into(),
        });
        win.dispatch_event(WindowEvent::KeyPressed { text: key.clone() });
        win.dispatch_event(WindowEvent::KeyReleased { text: key });
        win.dispatch_event(WindowEvent::KeyReleased {
            text: Key::Control.into(),
        });
    }

    /// One platform per process — both tests share the threaded testing
    /// backend (`threading` gives a real event-loop queue; `mock_time`
    /// keeps timers deterministic).
    fn testing_platform() {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
                i_slint_backend_testing::TestingBackendOptions {
                    mock_time: true,
                    threading: true,
                    renderer_name: Some("software".into()),
                },
            )))
            .expect("platform init");
        });
    }

    #[test]
    fn ui_smoke_keys_and_overlay() {
        testing_platform();

        let window = MainWindow::new().unwrap();
        window.show().unwrap();

        let seen = Rc::new(RefCell::new(Vec::<(String, bool, bool, bool)>::new()));
        let tree = Rc::new(RefCell::new(SchemaTree::default()));
        {
            let seen = Rc::clone(&seen);
            let tree = Rc::clone(&tree);
            let weak = window.as_weak();
            window
                .global::<Bridge>()
                .on_command(move |text, ctrl, shift, alt| {
                    let cmd =
                        commands::parse_command(&commands::key_string(&text, ctrl, shift, alt));
                    if cmd == Some(Command::OpenPalette) {
                        if let Some(win) = weak.upgrade() {
                            crate::bridge::open_palette(&win.global::<Bridge>(), &tree.borrow());
                        }
                    }
                    seen.borrow_mut().push((text.to_string(), ctrl, shift, alt));
                    cmd.is_some()
                });
        }

        // Palette callbacks, mirroring `run`'s wiring minus the UiCtx
        // (submit records the tag instead of running the command).
        let submitted = Rc::new(RefCell::new(Vec::<String>::new()));
        {
            let tree = Rc::clone(&tree);
            let weak = window.as_weak();
            window.global::<Bridge>().on_palette_edited(move |q| {
                if let Some(win) = weak.upgrade() {
                    let bridge = win.global::<Bridge>();
                    bridge.set_palette_items(palette_model(&tree.borrow(), &q));
                    bridge.set_palette_index(0);
                }
            });
        }
        {
            let submitted = Rc::clone(&submitted);
            let weak = window.as_weak();
            window.global::<Bridge>().on_palette_submit(move || {
                if let Some(win) = weak.upgrade() {
                    let bridge = win.global::<Bridge>();
                    let items = bridge.get_palette_items();
                    if let Some(item) = items.row_data(bridge.get_palette_index().max(0) as usize) {
                        submitted.borrow_mut().push(item.command.to_string());
                    }
                    bridge.set_palette_visible(false);
                }
            });
        }
        {
            let weak = window.as_weak();
            window.global::<Bridge>().on_palette_close(move || {
                if let Some(win) = weak.upgrade() {
                    win.global::<Bridge>().set_palette_visible(false);
                }
            });
        }

        let actions = Rc::new(RefCell::new(Vec::<String>::new()));
        {
            let actions = Rc::clone(&actions);
            window
                .global::<Bridge>()
                .on_completion_action(move |a| actions.borrow_mut().push(a.to_string()));
        }

        let win = window.window();
        for ch in "SELECT x".chars() {
            press(win, ch.to_string());
        }
        assert_eq!(
            window.global::<Bridge>().get_editor_text().as_str(),
            "SELECT x",
            "plain keys must insert into the editor (callback returned reject)"
        );

        // Open the completion popup (Rust-side wiring pushes this model
        // in the real app) — Ctrl+Return must still reach Bridge.command,
        // not be swallowed as an accept.
        window
            .global::<Bridge>()
            .set_completions(ModelRc::new(VecModel::from(vec![SharedString::from(
                "SELECTED",
            )])));

        // Ctrl+Return: modifier press + Return press.
        win.dispatch_event(WindowEvent::KeyPressed {
            text: Key::Control.into(),
        });
        win.dispatch_event(WindowEvent::KeyPressed {
            text: Key::Return.into(),
        });
        win.dispatch_event(WindowEvent::KeyReleased {
            text: Key::Return.into(),
        });
        win.dispatch_event(WindowEvent::KeyReleased {
            text: Key::Control.into(),
        });

        {
            let seen = seen.borrow();
            assert!(
                seen.iter().any(|(t, ctrl, _s, _a)| *ctrl && t == "\n"),
                "Ctrl+Enter must reach Bridge.command — saw {seen:?}"
            );
        }
        // The key was consumed: no newline inserted.
        assert_eq!(
            window.global::<Bridge>().get_editor_text().as_str(),
            "SELECT x"
        );
        // Popup was open but modified keys bypass it: no accept fired.
        assert!(
            actions.borrow().is_empty(),
            "Ctrl+Return must not accept a completion: {:?}",
            actions.borrow()
        );
        // Bare Return while the popup is open DOES accept.
        press(win, Key::Return);
        assert_eq!(actions.borrow().as_slice(), ["accept"]);

        // Push spans like editor-changed would, then rasterize.
        window
            .global::<Bridge>()
            .set_highlight_spans(spans_model("SELECT x"));
        let shot = win.take_snapshot().expect("snapshot");
        let (w, h) = (shot.width() as usize, shot.height() as usize);
        assert!(w > 400 && h > 300, "window size {w}x{h}");
        let px = shot.as_bytes();
        let at = |x: usize, y: usize| -> [u8; 3] {
            let i = (y * w + x) * 4;
            [px[i], px[i + 1], px[i + 2]]
        };
        let near =
            |p: [u8; 3], c: [u8; 3]| -> bool { p.iter().zip(c).all(|(a, b)| a.abs_diff(b) < 40) };
        // Catppuccin-mauve keyword pixels must appear somewhere in the
        // editor band (right of the sidebar, above the results pane).
        let mut mauve = 0usize;
        let mut mauve_pos = (0usize, 0usize);
        for y in 60..(h * 2 / 3) {
            for x in 280..w.saturating_sub(10) {
                if near(at(x, y), [0xCB, 0xA6, 0xF7]) {
                    mauve += 1;
                    mauve_pos = (x, y);
                }
            }
        }
        assert!(
            mauve > 20,
            "expected mauve keyword pixels in the editor area, found {mauve}"
        );
        // Alignment: mauve pixels must sit on the same text line as fg
        // pixels (within ~2px vertically of each other in the first rows).
        let text_row = mauve_pos.1;
        let mut fg_on_row = 0usize;
        for x in 280..280 + 600.min(w - 300) {
            if near(at(x, text_row + 6), [0xCD, 0xD6, 0xF4])
                || near(at(x, text_row), [0xCD, 0xD6, 0xF4])
            {
                fg_on_row += 1;
            }
        }
        assert!(
            fg_on_row > 0,
            "fg text should share the overlay's line (row {text_row})"
        );

        // Artifact for eyeballing: PPM next to target/.
        let mut ppm = format!("P6\n{w} {h}\n255\n").into_bytes();
        for y in 0..h {
            for x in 0..w {
                ppm.extend_from_slice(&at(x, y));
            }
        }
        let dir = std::path::Path::new("target/ui-smoke");
        std::fs::create_dir_all(dir).ok();
        std::fs::write(dir.join("ui-smoke-editor.ppm"), ppm).unwrap();

        // ── Palette smoke (Task 4.5) ───────────────────────────────
        // Ctrl+P opens the palette; typing fuzzy-filters; Return submits
        // the top row; Escape closes.
        press_ctrl(win, "p");
        let bridge = window.global::<Bridge>();
        assert!(bridge.get_palette_visible(), "Ctrl+P must open the palette");
        assert_eq!(bridge.get_palette_items().row_count(), 11 + 1);

        for ch in "exe".chars() {
            press(win, ch.to_string());
        }
        assert_eq!(bridge.get_palette_query().as_str(), "exe");
        let items = bridge.get_palette_items();
        assert_eq!(
            items.row_data(0).map(|i| i.command.to_string()).as_deref(),
            Some("execute"),
            "\"exe\" should rank \"Execute query\" first"
        );
        press(win, Key::Return);
        assert_eq!(submitted.borrow().as_slice(), ["execute"]);
        assert!(!bridge.get_palette_visible(), "submit closes the palette");

        // Reopen — Escape dismisses without submitting.
        press_ctrl(win, "p");
        assert!(bridge.get_palette_visible());
        assert_eq!(bridge.get_palette_query().as_str(), "");
        press(win, Key::Escape);
        assert!(!bridge.get_palette_visible(), "Escape closes the palette");
        assert_eq!(submitted.borrow().len(), 1);
        // ── Result grid smoke (Task 5.2) ────────────────────────────
        // Push a fake result through GridState (what AppEvent::QueryResult
        // calls in apply): columns + rows + footer land on the Bridge, and
        // the NULL cell carries is_null so .slint styles it.
        let grid = Rc::new(RefCell::new(crate::grid::GridState::default()));
        // Bridge callbacks wired exactly like `run()` so pointer/key events
        // dispatch through the real path.
        {
            let grid = Rc::clone(&grid);
            let weak = window.as_weak();
            bridge.on_grid_select(move |row, col| {
                let Some(win) = weak.upgrade() else { return };
                grid.borrow_mut().select(&win.global::<Bridge>(), row, col);
            });
        }
        {
            let grid = Rc::clone(&grid);
            let weak = window.as_weak();
            bridge.on_grid_drag(move |row, x| {
                let Some(win) = weak.upgrade() else { return };
                grid.borrow_mut().drag(&win.global::<Bridge>(), row, x);
            });
        }
        {
            let grid = Rc::clone(&grid);
            let weak = window.as_weak();
            bridge.on_resize_column(move |idx, delta| {
                let Some(win) = weak.upgrade() else { return };
                grid.borrow_mut()
                    .resize(&win.global::<Bridge>(), idx, delta);
            });
        }
        {
            let grid = Rc::clone(&grid);
            let weak = window.as_weak();
            bridge.on_sort_column(move |idx| {
                let Some(win) = weak.upgrade() else { return };
                grid.borrow_mut().sort(&win.global::<Bridge>(), idx);
            });
        }
        let copied = Rc::new(std::cell::Cell::new(false));
        {
            let grid = Rc::clone(&grid);
            let weak = window.as_weak();
            let copied = Rc::clone(&copied);
            let clip = Rc::new(RefCell::new(None));
            bridge.on_copy_selection(move || {
                copied.set(true);
                if let Some(win) = weak.upgrade() {
                    grid.borrow_mut()
                        .copy(&mut clip.borrow_mut(), &win.global::<Bridge>());
                }
            });
        }
        grid.borrow_mut().set_result(
            &bridge,
            datara_domain::QueryResult {
                columns: vec![
                    datara_domain::QueryColumn {
                        name: "id".into(),
                        data_type: "int".into(),
                    },
                    datara_domain::QueryColumn {
                        name: "note".into(),
                        data_type: "nvarchar".into(),
                    },
                ],
                rows: vec![
                    datara_domain::QueryRow {
                        cells: vec![
                            datara_domain::Value::Int(1),
                            datara_domain::Value::Text("x".into()),
                        ],
                    },
                    datara_domain::QueryRow {
                        cells: vec![datara_domain::Value::Int(2), datara_domain::Value::Null],
                    },
                ],
                rows_affected: None,
                truncated: false,
            },
            "2 rows".into(),
        );
        let cols = bridge.get_columns();
        assert_eq!(cols.row_count(), 2, "columns pushed");
        assert_eq!(cols.row_data(0).unwrap().name.as_str(), "id");
        assert_eq!(cols.row_data(1).unwrap().data_type.as_str(), "nvarchar");
        let rows = bridge.get_rows();
        assert_eq!(rows.row_count(), 2, "rows pushed");
        let r0 = rows.row_data(0).unwrap();
        assert_eq!(r0.cells.row_count(), 2);
        assert_eq!(r0.cells.row_data(0).unwrap().text.as_str(), "1");
        assert!(!r0.cells.row_data(0).unwrap().is_null);
        let r1 = rows.row_data(1).unwrap();
        assert!(
            r1.cells.row_data(1).unwrap().is_null,
            "Value::Null must map to is-null for the 'NULL' styling path"
        );
        assert_eq!(bridge.get_result_info().as_str(), "2 rows");

        // Real pointer drag: press on a cell, move, release — the
        // down-event must anchor the selection and the grabbed moved
        // events must extend it. Scan for a live cell first so the test
        // doesn't depend on hardcoded pixel geometry.
        let pt = |x: f64, y: f64| slint::LogicalPosition::new(x as f32, y as f32);
        let mut hit = None;
        'scan: for y in (440..680).step_by(4) {
            for x in (270..700).step_by(10) {
                win.dispatch_event(WindowEvent::PointerPressed {
                    position: pt(x as f64, y as f64),
                    button: slint::platform::PointerEventButton::Left,
                });
                win.dispatch_event(WindowEvent::PointerReleased {
                    position: pt(x as f64, y as f64),
                    button: slint::platform::PointerEventButton::Left,
                });
                if bridge.get_selection().active {
                    hit = Some((x, y));
                    break 'scan;
                }
            }
        }
        let Some((hx, hy)) = hit else {
            panic!("no grid cell received the pointer press");
        };
        let anchor = bridge.get_selection();
        // Extend the drag ~1.5 rows down and ~1.5 cols right of the anchor
        // cell; head row/col must differ from the anchor's.
        win.dispatch_event(WindowEvent::PointerPressed {
            position: pt(hx as f64, hy as f64),
            button: slint::platform::PointerEventButton::Left,
        });
        win.dispatch_event(WindowEvent::PointerMoved {
            position: pt(hx as f64 + 160.0, hy as f64 + 30.0),
        });
        win.dispatch_event(WindowEvent::PointerMoved {
            position: pt(hx as f64 + 200.0, hy as f64 + 36.0),
        });
        let sel = bridge.get_selection();
        assert!(
            sel.active && (sel.r1 > sel.r0 || sel.c1 > sel.c0),
            "drag must extend the selection head past the anchor: {sel:?}"
        );
        win.dispatch_event(WindowEvent::PointerReleased {
            position: pt(hx as f64 + 200.0, hy as f64 + 36.0),
            button: slint::platform::PointerEventButton::Left,
        });
        // 2 rows × 140px columns: anchor (0,0) at scan granularity must
        // land within the 2×2 result — ends normalize to (0,0)-(1,1).
        assert_eq!((anchor.r0, anchor.c0), (0, 0));
        assert_eq!((sel.r1, sel.c1), (1, 1));

        // The press focused the grid's FocusScope — Ctrl+C must reach
        // copy-selection (clipboard itself may be absent headless; the
        // callback firing is the contract).
        assert!(!copied.get());
        press_ctrl(win, "c");
        assert!(copied.get(), "Ctrl+C on the grid must fire copy-selection");

        // Column-resize drag on the 4px handle at a column's right edge:
        // press, move +40, release → the commit lands once on pointer-up.
        // Scan the header band for the handle (drag that changes a width),
        // avoiding geometry guesses.
        let w_before = bridge.get_columns().row_data(0).unwrap().width;
        let mut resized = None;
        'hscan: for hy in (470..510).step_by(2) {
            // col0 edge ≈ sidebar(264) + 140; probe ±8px around it.
            for hx in (396..=412).step_by(2) {
                win.dispatch_event(WindowEvent::PointerPressed {
                    position: pt(hx as f64, hy as f64),
                    button: slint::platform::PointerEventButton::Left,
                });
                win.dispatch_event(WindowEvent::PointerMoved {
                    position: pt(hx as f64 + 20.0, hy as f64),
                });
                win.dispatch_event(WindowEvent::PointerMoved {
                    position: pt(hx as f64 + 40.0, hy as f64),
                });
                // Still uncommitted mid-drag.
                assert_eq!(
                    bridge.get_columns().row_data(0).unwrap().width,
                    w_before,
                    "resize must not commit until pointer-up"
                );
                win.dispatch_event(WindowEvent::PointerReleased {
                    position: pt(hx as f64 + 40.0, hy as f64),
                    button: slint::platform::PointerEventButton::Left,
                });
                let w = bridge.get_columns().row_data(0).unwrap().width;
                if w != w_before {
                    resized = Some(w);
                    break 'hscan;
                }
            }
        }
        assert_eq!(
            resized,
            Some(w_before + 40),
            "handle drag must commit +40px on release"
        );

        // Header-click sort via the model path (pointer-level covered by
        // select/resize): descending flips row order in the pushed model.
        grid.borrow_mut().sort(&bridge, 0);
        grid.borrow_mut().sort(&bridge, 0);
        assert_eq!(bridge.get_sort_col(), 0);
        assert!(!bridge.get_sort_asc(), "second click flips direction");
        let rows = bridge.get_rows();
        assert_eq!(
            rows.row_data(0)
                .unwrap()
                .cells
                .row_data(0)
                .unwrap()
                .text
                .as_str(),
            "2",
            "descending sort puts id=2 first"
        );
        // Sort cleared the selection (coords refer to old row order).
        assert!(!bridge.get_selection().active);

        // Column resize clamps at the 40px minimum; bad index is rejected.
        grid.borrow_mut().resize(&bridge, 0, -1000.0);
        assert_eq!(bridge.get_columns().row_data(0).unwrap().width, 40);
        grid.borrow_mut().resize(&bridge, -1, 500.0);
        assert_eq!(
            bridge.get_columns().row_data(0).unwrap().width,
            40,
            "negative idx must not touch column 0"
        );

        // Snapshot while the grid is populated — the cell region must
        // differ from the empty pane (header bar + cell text land pixels).
        let shot = win.take_snapshot().expect("grid snapshot");
        let (w, h) = (shot.width() as usize, shot.height() as usize);
        let px = shot.as_bytes();
        let at = |x: usize, y: usize| -> [u8; 3] {
            let i = (y * w + x) * 4;
            [px[i], px[i + 1], px[i + 2]]
        };
        // The results pane is the bottom third of the window; the sorted
        // grid paints fg-colored text there.
        let mut fg_px = 0usize;
        for y in (h * 2 / 3)..(h - 30) {
            for x in 280..w.saturating_sub(10) {
                let p = at(x, y);
                if p.iter()
                    .zip([0xCD, 0xD6, 0xF4])
                    .all(|(a, b)| a.abs_diff(b) < 30)
                {
                    fg_px += 1;
                }
            }
        }
        assert!(
            fg_px > 20,
            "expected grid text pixels in the results pane, found {fg_px}"
        );
        // QueryStarted path: clear empties everything but keeps info text.
        grid.borrow_mut().clear(&bridge, "Running…");
        assert_eq!(bridge.get_rows().row_count(), 0);
        assert_eq!(bridge.get_columns().row_count(), 0);
        assert_eq!(bridge.get_result_info().as_str(), "Running…");

        // ── History panel smoke (Task 6.1) ─────────────────────────
        // history-toggle (toolbar/Ctrl+Shift+F entry) flips visibility and
        // fires the open search; the pushed model lands on
        // Bridge.history-items.
        let searches = Rc::new(RefCell::new(Vec::<String>::new()));
        {
            let searches = Rc::clone(&searches);
            bridge.on_history_search(move |f| searches.borrow_mut().push(f.to_string()));
        }
        {
            // Same shape as run()'s on_history_toggle, minus UiCtx.
            let weak = window.as_weak();
            bridge.on_history_toggle(move || {
                if let Some(win) = weak.upgrade() {
                    let b = win.global::<Bridge>();
                    let open = !b.get_history_visible();
                    b.set_history_visible(open);
                    if open {
                        b.invoke_history_search(b.get_history_query());
                    }
                }
            });
        }
        bridge.invoke_history_toggle();
        assert!(bridge.get_history_visible());
        assert_eq!(searches.borrow().as_slice(), [""]);
        bridge.set_history_items(ModelRc::new(VecModel::from(vec![crate::HistoryItem {
            id: 1,
            query: "select 1".into(),
            started_at: "2026-09-28 10:00".into(),
            duration: "12ms".into(),
            rows: "1 rows".into(),
            ok: true,
        }])));
        let items = bridge.get_history_items();
        assert_eq!(items.row_count(), 1);
        assert_eq!(items.row_data(0).unwrap().query.as_str(), "select 1");
        bridge.invoke_history_toggle();
        assert!(!bridge.get_history_visible());
    }

    /// Live Task 5.3 smoke: `AppEvent::PreviewSql` — what double-clicking a
    /// table dispatches — opens a tab AND executes through the same
    /// Ctrl+Enter path, landing rows in the grid. Needs datara-mssql on
    /// 127.0.0.1:11433 (sa / Datara!1234) and a session Secret Service.
    #[test]
    #[ignore = "needs datara-mssql on 127.0.0.1:11433 and a session Secret Service"]
    fn ui_smoke_preview_executes_into_grid() {
        use datara_domain::{AuthenticationMode, EncryptionMode};
        use datara_storage::NewConnection;
        use secrecy::SecretString;
        use std::time::{Duration, Instant};

        // The event loop only runs on the thread that installed the
        // platform — keep this test `#[ignore]`d so nothing else claims
        // `set_platform` first.
        testing_platform();

        let tmp = std::env::temp_dir().join(format!("datara-preview-smoke-{}", std::process::id()));
        std::env::set_var("DATARA_DATA_DIR", &tmp);
        std::env::set_var("DATARA_CONFIG_DIR", tmp.join("cfg"));
        std::env::set_var("DATARA_STATE_DIR", tmp.join("state"));

        let svc = AppServices::init().expect("services init");
        let (id, reference) = svc.runtime.block_on(async {
            let repo = svc.backend.storage.connections();
            let id = repo
                .insert(NewConnection {
                    name: "preview-smoke".into(),
                    host: "127.0.0.1".into(),
                    port: 11433,
                    database: Some("master".into()),
                    username: "sa".into(),
                    authentication: AuthenticationMode::SqlPassword,
                    encryption: EncryptionMode::Preferred,
                    trust_server_certificate: true,
                })
                .await
                .unwrap();
            let reference = repo.get(id).await.unwrap().secret_reference;
            svc.backend
                .secrets
                .save(
                    &reference,
                    "preview-smoke",
                    &SecretString::from("Datara!1234".to_owned()),
                )
                .await
                .unwrap();
            (id, reference)
        });

        let cx = Arc::new(UiCtx {
            tree: Mutex::new(SchemaTree::default()),
            grid: Mutex::new(crate::grid::GridState::default()),
            editor: Mutex::new(EditorState::default()),
            backend: Arc::clone(&svc.backend),
            handle: svc.runtime.handle().clone(),
            query_limit: svc.config.query.default_limit,
            clipboard: Mutex::new(None),
        });
        let window = MainWindow::new().unwrap();
        let bridge = window.global::<Bridge>();

        // Exactly what on_open_table produces for a sys.tables node.
        crate::bridge::apply(
            &window,
            &cx,
            AppEvent::PreviewSql {
                conn_id: id,
                database: Some("master".into()),
                sql: crate::services::preview_sql("sys", "tables", 5),
                label: "sys.tables".into(),
            },
        );
        assert_eq!(
            bridge.get_editor_text().as_str(),
            "SELECT TOP 5 * FROM [sys].[tables]",
            "preview SQL must land in the new tab"
        );
        // The spawn happened synchronously — proof the PreviewSql arm ran
        // the same execute entry as Ctrl+Enter (run_command → ExecuteQuery).
        assert_eq!(bridge.get_status().as_str(), "Executing…");

        // Terminal events return through UiHandle::dispatch → the event
        // loop, so run it on this thread; a poll thread watches the
        // Rust-side GridState and quits once the result is applied (the
        // running-slot flag drops before the event is dispatched, so it
        // can't serve as the done signal).
        {
            let cx = Arc::clone(&cx);
            std::thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(60);
                while Instant::now() < deadline {
                    if cx.grid.lock().cached_rows() > 0 {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
                slint::quit_event_loop().unwrap();
            });
        }
        slint::run_event_loop().unwrap();

        assert_eq!(cx.grid.lock().cached_rows(), 5, "preview must execute");
        assert_eq!(bridge.get_rows().row_count(), 5);
        assert!(!bridge.get_status().is_empty());

        svc.runtime
            .block_on(svc.backend.secrets.delete(&reference))
            .unwrap();
        std::fs::remove_dir_all(&tmp).ok();
    }
}
