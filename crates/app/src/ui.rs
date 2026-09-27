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

use crate::bridge::{catalog_labels, push_tabs, push_tree, spans_model, AppEvent, UiCtx, UiHandle};
use crate::commands;
use crate::editor_ui::{line_col, line_count, EditorState};
use crate::schema_tree::SchemaTree;
use crate::services::AppServices;
use crate::{Bridge, MainWindow};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

/// Build the window, attach callbacks, load the initial sidebar list, run.
pub fn run(services: AppServices) -> anyhow::Result<()> {
    let window = MainWindow::new()?;
    tracing::debug!(theme = %services.config.appearance.theme, "loaded config");
    let cx = Arc::new(UiCtx {
        tree: Mutex::new(SchemaTree::default()),
        editor: Mutex::new(EditorState::default()),
        backend: Arc::clone(&services.backend),
        handle: services.runtime.handle().clone(),
        query_limit: services.config.query.default_limit,
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
        let services = Rc::clone(&services);
        let ui = ui.clone();
        let cx = Arc::clone(&cx);
        let weak = window.as_weak();
        bridge.on_toggle_node(move |id| {
            let backend = services.backend.clone();
            let ui = ui.clone();
            let node = {
                let Some(win) = weak.upgrade() else {
                    return;
                };
                let bridge = win.global::<Bridge>();
                let mut tree = cx.tree.lock();
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

    window.run()?;
    Ok(())
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
    use slint::platform::{Key, WindowEvent};
    use std::cell::RefCell;

    fn press(win: &slint::Window, text: impl Into<slint::SharedString>) {
        let text = text.into();
        win.dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
        win.dispatch_event(WindowEvent::KeyReleased { text });
    }

    #[test]
    fn ui_smoke_keys_and_overlay() {
        // One platform per process: mock time + event loop + real
        // software rasterizer so take_snapshot produces pixels.
        slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
            i_slint_backend_testing::TestingBackendOptions {
                mock_time: true,
                threading: true,
                renderer_name: Some("software".into()),
            },
        )))
        .expect("platform init");

        let window = MainWindow::new().unwrap();
        window.show().unwrap();

        let seen = Rc::new(RefCell::new(Vec::<(String, bool, bool, bool)>::new()));
        {
            let seen = Rc::clone(&seen);
            window
                .global::<Bridge>()
                .on_command(move |text, ctrl, shift, alt| {
                    let cmd =
                        commands::parse_command(&commands::key_string(&text, ctrl, shift, alt));
                    seen.borrow_mut().push((text.to_string(), ctrl, shift, alt));
                    cmd.is_some()
                });
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
    }
}
