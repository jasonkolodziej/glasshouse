//! Glasshouse: a terminal drawn on its own character grid, whose trust signals come from the kernel.
//!
//! ```sh
//! cargo run -p glasshouse                                   # a live shell
//! cargo run -p glasshouse -- --demo                         # scripted tabs, no shell (screenshots, demos)
//! cargo run -p glasshouse -- --snap shot.png --scene palette --scale 2 --at 0.5
//! ```
//!
//! Scenes (`--scene NAME`): `terminal`, `sidebar`, `palette`, `approval`, `find`, `settings`,
//! `ssh`, `appearance`, `light` and `solid`. Each fills the window with demo tabs, so no shell is spawned.

mod app;
mod appearance;
mod demo;
mod hosts;
mod notify;
mod palette;
mod session;
mod shortcuts;
mod ui;

use rune::{AppSpec, KeyEvent, Tx, Waker, WindowOptions};

use crate::app::App;

pub const SIZE: (f32, f32) = (1180.0, 740.0);

fn setup(app: &mut App, waker: Waker) {
    // Snapshots, scripts and traces draw scenes; they never spawn a shell.
    if rune::is_headless() {
        return;
    }
    // A window on screen: glass is the system material over the real desktop.
    app.native_glass = true;
    if rune::extra_args().iter().any(|a| a == "--demo") {
        demo::terminal(app);
        return;
    }
    app.setup(waker);
}

fn poll(app: &mut App, clock: f64, tx: &mut Tx) -> bool {
    app.poll(clock, tx)
}

fn on_key(app: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
    app.on_key(ev, tx)
}

fn main() {
    rune::set_app_id("glasshouse");
    rune::App::new(AppSpec {
        min_size: Some((640.0, 420.0)),
        scenes: demo::SCENES,
        families: ("Inter", ui::MONO_FAMILIES),
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
        ..AppSpec::new("Glasshouse", SIZE, App::default, ui::view)
    })
    // Transparent, so below 100% opacity the desktop shows through Liquid Glass. A window's
    // transparency is fixed when it opens, so it is always on: at 100% the frame paints the
    // theme's background over every pixel, which looks exactly like an opaque window.
    .window_options(WindowOptions {
        transparent: true,
        ..WindowOptions::default()
    })
    .run();
}

#[cfg(test)]
mod tests {
    use rune::{Env, Runtime};

    use super::*;

    fn scene(name: &str) -> Runtime<App> {
        scene_with(name, |_| {})
    }

    /// A scene with `tweak` applied before its first frame.
    fn scene_with(name: &str, tweak: impl FnOnce(&mut App)) -> Runtime<App> {
        let mut app = App::new(appearance::ThemeLibrary::built_in());
        let (_, apply) = demo::SCENES
            .iter()
            .find(|(n, _)| *n == name)
            .expect("scene exists");
        apply(&mut app);
        tweak(&mut app);
        let mut rt = Runtime::new(app, ui::view, SIZE, Env::new(true, 1.0));
        rt.frame(0.0);
        rt.frame(1.0 / 60.0);
        rt
    }

    #[test]
    fn every_scene_should_draw() {
        for (name, _) in demo::SCENES {
            let rt = scene(name);
            assert!(!rt.instances.is_empty(), "{name} drew nothing");
        }
    }

    #[test]
    fn glass_on_screen_should_show_the_desktop_not_the_painted_wallpaper() {
        // Headless (snapshots, README pictures): no desktop, so a painted wallpaper, blurred.
        let headless = scene("terminal");
        assert!(headless.find("wallpaper").is_some());
        assert!(headless.find("frost").is_some());

        // A window on screen: the system's Liquid Glass shows the real desktop instead.
        let on_screen = scene_with("terminal", |app| app.native_glass = true);
        assert!(on_screen.find("window").is_some());
        assert!(on_screen.find("wallpaper").is_none());
        assert!(on_screen.find("frost").is_none());

        // At 100% opacity neither draws: the theme's background fills the window.
        let opaque = scene_with("solid", |app| app.native_glass = true);
        assert!(opaque.find("wallpaper").is_none());
        assert!(opaque.find("frost").is_none());
    }

    #[test]
    fn picking_a_theme_should_recolour_open_tabs() {
        let mut rt = scene("terminal");
        rt.poll = Some(poll);
        let before = rt.state.selected().expect("a tab").appearance.background;
        rt.state.update_appearance(|c| {
            c.theme.dark = "high-contrast-dark".into();
            c.theme.light = "high-contrast-dark".into();
        });
        rt.invalidate();
        rt.frame(2.0 / 60.0);
        let after = rt.state.selected().expect("a tab").appearance.background;
        assert_ne!(before, after);
        assert_eq!(
            (after.r, after.g, after.b),
            (0.0, 0.0, 0.0),
            "High Contrast Dark is black"
        );
    }

    #[test]
    fn layout_should_size_the_grid_to_the_pane() {
        let rt = scene("terminal");
        let size = rt.state.grid_size().expect("cell and pane measured");
        assert!(size.cols > 80 && size.rows > 20, "{size:?}");
    }

    #[test]
    fn pressing_a_tab_should_select_it() {
        let mut rt = scene("terminal");
        let second = rt.state.sessions[1].id;
        let (x, y) = rt.find(&format!("tab-{second}")).expect("tab on screen");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(2.0 / 60.0);
        assert_eq!(rt.state.selected, Some(second));
    }
}
