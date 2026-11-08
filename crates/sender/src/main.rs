//! Mac app that streams a RadioMaster TX16S to a PC, where `tx16s-receiver`
//! turns it into a vJoy joystick for games played over Steam/Moonlight streaming.
//!
//! The app lives in the menu bar: closing the window keeps the stream running.

mod bridge;
mod config;
mod engine;
mod radio;
#[cfg(target_os = "macos")]
mod status_bar;
mod text_input;
mod theme;
mod ui;

use gpui::{
    App, Application, Bounds, Entity, Focusable, Global, KeyBinding, Menu, MenuItem,
    TitlebarOptions, WindowBounds, WindowHandle, WindowOptions, actions, point, prelude::*, px,
    size,
};

use crate::bridge::Bridge;
use crate::ui::BridgeWindow;

actions!(tx16s_bridge, [Quit, CloseWindow]);

/// The stream model and the window showing it, if open.
struct Shell {
    bridge: Entity<Bridge>,
    window: Option<WindowHandle<BridgeWindow>>,
}

impl Global for Shell {}

fn main() {
    let app = Application::new();
    // Clicking the Dock icon brings the window back.
    app.on_reopen(show_window);
    app.run(|cx: &mut App| {
        text_input::bind_keys(cx);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_action(|_: &CloseWindow, cx| {
            if let Some(window) = cx.active_window() {
                let _ = window.update(cx, |_, window, _| window.remove_window());
            }
        });
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-w", CloseWindow, None),
        ]);
        cx.set_menus(vec![Menu {
            name: "TX16S Bridge".into(),
            items: vec![
                MenuItem::action("Fermer la fenêtre", CloseWindow),
                MenuItem::separator(),
                MenuItem::action("Quitter TX16S Bridge", Quit),
            ],
        }]);

        let bridge = cx.new(Bridge::new);
        cx.set_global(Shell { bridge: bridge.clone(), window: None });
        show_window(cx);

        #[cfg(target_os = "macos")]
        {
            status_bar::install(bridge, show_window, cx);
            // Without a window, the app only lives in the menu bar.
            cx.on_window_closed(|cx| {
                if cx.windows().is_empty() {
                    status_bar::set_dock_icon_visible(false);
                }
            })
            .detach();
        }
        #[cfg(not(target_os = "macos"))]
        cx.on_window_closed(|cx| cx.quit()).detach();

        cx.activate(true);
    });
}

/// Brings the window to the front, reopening it if it was closed.
fn show_window(cx: &mut App) {
    #[cfg(target_os = "macos")]
    status_bar::set_dock_icon_visible(true);
    cx.activate(true);

    let Shell { bridge, window } = cx.global::<Shell>();
    let (bridge, existing) = (bridge.clone(), *window);
    if let Some(handle) = existing
        && handle.update(cx, |_, window, _| window.activate_window()).is_ok()
    {
        return;
    }

    let bounds = Bounds::centered(None, size(px(580.), px(800.)), cx);
    let handle = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("TX16S Bridge".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(14.), px(14.))),
                }),
                window_min_size: Some(size(px(520.), px(800.))),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| BridgeWindow::new(bridge, cx));
                window.focus(&view.focus_handle(cx));
                view
            },
        )
        .expect("failed to open the window");
    cx.global_mut::<Shell>().window = Some(handle);
}
