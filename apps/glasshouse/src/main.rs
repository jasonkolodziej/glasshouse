//! Glasshouse: a terminal drawn on its own character grid, whose trust signals come from the kernel.
//!
//! ```sh
//! cargo run -p glasshouse                                   # a live shell
//! cargo run -p glasshouse -- --demo                         # scripted tabs, no shell (screenshots, demos)
//! cargo run -p glasshouse -- --snap shot.png --scene palette --scale 2 --at 0.5
//! ```
//!
//! Scenes (`--scene NAME`): `terminal`, `sidebar`, `palette`, `approval`, `find`, `settings`,
//! `ssh`, `appearance`, `light`, `solid`, `hero`, `split`, `stacked`, `arranged` and `dragging`. Each fills the
//! window with demo tabs, so no shell is spawned.

mod app;
mod appearance;
mod demo;
mod hosts;
mod notify;
mod palette;
mod panes;
mod session;
mod shortcuts;
mod ui;

use rune::{AppSpec, Chrome, KeyEvent, Tx, Waker, WindowOptions};

use crate::app::App;

pub const SIZE: (f32, f32) = (1180.0, 740.0);

/// The app icon (`AppIconC`, "Glasshouse"), 1024 px. A bundled release also carries it as
/// `AppIcon.icns`; setting it at runtime covers `cargo run` and the bare binary.
const ICON: &[u8] = include_bytes!("../../../docs/icon.png");

/// The window under the drawn title row. Only the row's `.window_drag()` gaps move the window:
/// without `drag_regions`, macOS drags from its whole titlebar band, and a tab pressed to be
/// dragged along the strip or onto a pane would move the window instead.
const CHROME: Chrome = Chrome {
    drag_regions: true,
    ..Chrome::DEFAULT
};
const _: () = assert!(
    CHROME.drag_regions && CHROME.full_size_content,
    "tabs must keep their presses to be dragged"
);

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
    if let Err(error) = rune::set_app_icon(ICON) {
        eprintln!("Glasshouse: {error}");
    }
    rune::App::new(AppSpec {
        min_size: Some((640.0, 420.0)),
        scenes: demo::SCENES,
        families: ("Inter", ui::MONO_FAMILIES),
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
        ..AppSpec::new("Glasshouse", SIZE, App::default, ui::view)
    })
    .chrome(CHROME)
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
    fn rail_icon_size_should_grow_the_rail_and_its_buttons() {
        let rect = |rt: &Runtime<App>, key: &str| rt.presented(key).expect(key).0;
        let default = scene("terminal");
        let large = scene_with("terminal", |app| {
            app.themes.config.rail_icon_size = Some(22.0);
        });
        assert_eq!(
            rect(&default, "rail")[2],
            40.0,
            "the default rail is unchanged"
        );
        assert_eq!(rect(&default, "new-tab")[2..], [30.0, 28.0]);
        assert_eq!(rect(&large, "rail")[2], 48.0);
        assert_eq!(rect(&large, "new-tab")[2..], [38.0, 36.0]);
        // The wider rail comes out of the terminal, not off the window.
        let (default_grid, large_grid) = (
            default.state.grid_size().expect("grid"),
            large.state.grid_size().expect("grid"),
        );
        assert!(large_grid.cols < default_grid.cols);
    }

    #[test]
    fn settings_should_offer_every_rail_icon_size() {
        let rt = scene("appearance");
        for name in ["Small", "Medium", "Large", "Huge"] {
            assert!(
                rt.find(&format!("rail-icon-size-{name}")).is_some(),
                "{name} missing from Settings → Appearance"
            );
        }
    }

    #[test]
    fn layout_should_size_the_grid_to_the_pane() {
        let rt = scene("terminal");
        let size = rt.state.grid_size().expect("cell and pane measured");
        assert!(size.cols > 80 && size.rows > 20, "{size:?}");
    }

    /// Each tab's grid size, in tab order.
    fn grids(rt: &Runtime<App>) -> Vec<(u16, u16)> {
        rt.state
            .sessions
            .iter()
            .map(|s| {
                let size = s.engine().size();
                (size.cols, size.rows)
            })
            .collect()
    }

    #[test]
    fn split_side_by_side_should_give_every_tab_a_pane_across_the_window() {
        let single = scene("terminal");
        let split = scene("split");
        let (full_cols, full_rows) = grids(&single)[0];
        for (i, s) in split.state.sessions.iter().enumerate() {
            assert!(
                split.find(&format!("pane-{}", s.id)).is_some(),
                "tab {i} has no pane"
            );
        }
        let panes = grids(&split);
        for (cols, rows) in &panes {
            assert!(*cols < full_cols / 2, "{panes:?} vs {full_cols} columns");
            assert_eq!(*rows, full_rows, "side by side keeps the height");
        }
        // Panes are left to right.
        let xs: Vec<f32> = split
            .state
            .sessions
            .iter()
            .map(|s| rect(&split, &format!("pane-{}", s.id))[0])
            .collect();
        assert!(xs.windows(2).all(|w| w[0] < w[1]), "{xs:?}");
    }

    #[test]
    fn split_stacked_should_give_every_tab_a_pane_down_the_window() {
        let single = scene("terminal");
        let stacked = scene("stacked");
        let (full_cols, full_rows) = grids(&single)[0];
        let panes = grids(&stacked);
        for (cols, rows) in &panes {
            assert_eq!(*cols, full_cols, "stacked keeps the width");
            assert!(*rows < full_rows / 2, "{panes:?} vs {full_rows} rows");
        }
        let ys: Vec<f32> = stacked
            .state
            .sessions
            .iter()
            .map(|s| rect(&stacked, &format!("pane-{}", s.id))[1])
            .collect();
        assert!(ys.windows(2).all(|w| w[0] < w[1]), "{ys:?}");
    }

    #[test]
    fn one_tab_on_screen_should_show_only_the_selected_pane() {
        let rt = scene("terminal");
        let shown: Vec<u64> = rt
            .state
            .sessions
            .iter()
            .map(|s| s.id)
            .filter(|id| rt.find(&format!("pane-{id}")).is_some())
            .collect();
        assert_eq!(shown, rt.state.selected.into_iter().collect::<Vec<_>>());
    }

    #[test]
    fn clicking_a_pane_should_give_it_the_keyboard() {
        let mut rt = scene("split");
        let last = rt.state.sessions[2].id;
        assert_ne!(rt.state.selected, Some(last));
        let (x, y) = rt.find(&format!("pane-{last}")).expect("pane on screen");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(2.0 / 60.0);
        assert_eq!(rt.state.selected, Some(last));
    }

    #[test]
    fn unsplitting_should_give_every_tab_the_whole_pane_again() {
        let single = grids(&scene("terminal"));
        let mut rt = scene("split");
        rt.state.set_layout(app::Layout::Single);
        rt.invalidate();
        rt.frame(2.0 / 60.0);
        rt.frame(3.0 / 60.0);
        assert_eq!(grids(&rt), single);
    }

    #[test]
    fn the_rail_should_offer_the_next_layout() {
        let mut rt = scene("terminal");
        for expected in [
            app::Layout::SideBySide,
            app::Layout::Stacked,
            app::Layout::Single,
        ] {
            let (x, y) = rt.find("layout").expect("layout button");
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
            rt.frame(2.0 / 60.0);
            assert_eq!(rt.state.layout(), expected);
        }
    }

    /// Presses at `from`, drags to `to` in a few steps and lets go, a frame after each.
    fn drag(rt: &mut Runtime<App>, from: (f32, f32), to: (f32, f32)) {
        let mut t = 2.0;
        let mut frame = |rt: &mut Runtime<App>| {
            t += 1.0;
            rt.frame(t / 60.0);
        };
        rt.pointer_down(from.0, from.1);
        frame(rt);
        for step in 1..=4 {
            let f = step as f32 / 4.0;
            rt.pointer_move(from.0 + (to.0 - from.0) * f, from.1 + (to.1 - from.1) * f);
            frame(rt);
        }
        rt.pointer_up(to.0, to.1);
        frame(rt);
        frame(rt);
    }

    /// Presses at `from` and drags to `to` without letting go.
    fn hold(rt: &mut Runtime<App>, from: (f32, f32), to: (f32, f32)) {
        rt.pointer_down(from.0, from.1);
        rt.frame(3.0 / 60.0);
        for step in 1..=4 {
            let f = step as f32 / 4.0;
            rt.pointer_move(from.0 + (to.0 - from.0) * f, from.1 + (to.1 - from.1) * f);
            rt.frame((3.0 + f64::from(step)) / 60.0);
        }
    }

    #[test]
    fn dragging_the_tab_on_screen_to_an_edge_should_split_it_with_its_neighbour() {
        let mut rt = scene("terminal");
        let [first, server, _] = ids(&rt);
        let pane = rect(&rt, &format!("pane-{first}"));
        let tab = rt.find(&format!("tab-{first}")).expect("tab");
        let bottom = (pane[0] + pane[2] / 2.0, pane[1] + pane[3] - 20.0);
        hold(&mut rt, tab, bottom);
        // While held, the half it would take is shaded, as VS Code shades a terminal's.
        let marker = rect(&rt, "drop-marker");
        assert!(
            (marker[3] - pane[3] / 2.0).abs() < 1.0 && marker[1] > pane[1],
            "{marker:?}"
        );
        rt.pointer_up(bottom.0, bottom.1);
        rt.frame(10.0 / 60.0);
        rt.frame(11.0 / 60.0);
        let tree = rt.state.split.clone().expect("a split");
        assert_eq!(
            tree.leaves(),
            [server, first],
            "the neighbour above, the tab below"
        );
        assert_eq!(tree.flat_axis(), Some(panes::Axis::Column));
        assert_eq!(rt.state.selected, Some(first));
        assert!(
            rt.find("drop-marker").is_none(),
            "the shading goes with the drop"
        );
    }

    #[test]
    fn a_drop_that_would_change_nothing_should_shade_nothing() {
        let mut rt = scene("terminal");
        let [first, ..] = ids(&rt);
        let pane = rect(&rt, &format!("pane-{first}"));
        let tab = rt.find(&format!("tab-{first}")).expect("tab");
        // The middle of its own pane: nothing to swap with.
        hold(
            &mut rt,
            tab,
            (pane[0] + pane[2] / 2.0, pane[1] + pane[3] / 2.0),
        );
        assert!(rt.state.tab_drag.is_some(), "the tab is lifted");
        assert!(rt.find("drop-marker").is_none());
    }

    fn ids(rt: &Runtime<App>) -> [u64; 3] {
        [0, 1, 2].map(|i| rt.state.sessions[i].id)
    }

    #[test]
    fn dropping_a_tab_on_a_panes_right_edge_should_put_it_beside_that_pane() {
        let mut rt = scene("terminal");
        let [first, server, _] = ids(&rt);
        let pane = rect(&rt, &format!("pane-{first}"));
        let tab = rt.find(&format!("tab-{server}")).expect("tab");
        drag(
            &mut rt,
            tab,
            (pane[0] + pane[2] - 20.0, pane[1] + pane[3] / 2.0),
        );
        let tree = rt.state.split.clone().expect("a split");
        assert_eq!(tree.leaves(), [first, server]);
        assert_eq!(tree.flat_axis(), Some(panes::Axis::Row));
        assert_eq!(rt.state.selected, Some(server), "the dropped tab has focus");
        assert_eq!(rt.state.layout(), app::Layout::Custom, "two of three tabs");
        assert!(rt.state.tab_drag.is_none());
        // Both panes on screen, the dropped one to the right.
        assert!(rect(&rt, &format!("pane-{server}"))[0] > rect(&rt, &format!("pane-{first}"))[0]);
    }

    #[test]
    fn dropping_a_tab_on_a_split_panes_bottom_edge_should_stack_it_there() {
        let mut rt = scene("split");
        let [first, server, notes] = ids(&rt);
        let pane = rect(&rt, &format!("pane-{server}"));
        let tab = rt.find(&format!("tab-{notes}")).expect("tab");
        drag(
            &mut rt,
            tab,
            (pane[0] + pane[2] / 2.0, pane[1] + pane[3] - 20.0),
        );
        let tree = rt.state.split.clone().expect("a split");
        assert_eq!(tree.leaves(), [first, server, notes]);
        let (s, n) = (
            rect(&rt, &format!("pane-{server}")),
            rect(&rt, &format!("pane-{notes}")),
        );
        assert!(
            (s[0] - n[0]).abs() < 1.0 && n[1] > s[1],
            "{s:?} above {n:?}"
        );
    }

    #[test]
    fn dragging_a_tab_along_the_strip_should_reorder_the_tabs_and_a_flat_split() {
        let mut rt = scene("split");
        let [first, server, notes] = ids(&rt);
        let first_tab = rect(&rt, &format!("tab-{first}"));
        let tab = rt.find(&format!("tab-{notes}")).expect("tab");
        drag(&mut rt, tab, (first_tab[0] + 4.0, tab.1));
        assert_eq!(ids(&rt), [notes, first, server]);
        assert_eq!(
            rt.state.split.as_ref().map(panes::PaneTree::leaves),
            Some(vec![notes, first, server])
        );
    }

    #[test]
    fn a_click_on_a_tab_should_still_select_it_without_moving_anything() {
        let mut rt = scene("terminal");
        let [_, server, _] = ids(&rt);
        let tab = rt.find(&format!("tab-{server}")).expect("tab");
        drag(&mut rt, tab, (tab.0 + 1.0, tab.1));
        assert_eq!(rt.state.selected, Some(server));
        assert!(rt.state.split.is_none());
    }

    #[test]
    fn dragging_a_divider_should_resize_the_panes_beside_it() {
        let mut rt = scene("split");
        let [first, server, notes] = ids(&rt);
        let before: Vec<f32> = [first, server, notes]
            .iter()
            .map(|id| rect(&rt, &format!("pane-{id}"))[2])
            .collect();
        let cols_before = grids(&rt)[0].0;
        let divider = rt.find("divider-root-0").expect("divider");
        drag(&mut rt, divider, (divider.0 + 120.0, divider.1));
        let after: Vec<f32> = [first, server, notes]
            .iter()
            .map(|id| rect(&rt, &format!("pane-{id}"))[2])
            .collect();
        assert!(
            (after[0] - before[0] - 120.0).abs() < 2.0,
            "{before:?} → {after:?}"
        );
        assert!(
            (before[1] - after[1] - 120.0).abs() < 2.0,
            "{before:?} → {after:?}"
        );
        assert!(
            (after[2] - before[2]).abs() < 1.0,
            "the third pane keeps its width"
        );
        assert!(
            grids(&rt)[0].0 > cols_before,
            "the wider pane's grid has more columns"
        );
    }

    #[test]
    fn the_arranged_scene_should_nest_a_column_beside_a_pane() {
        let rt = scene("arranged");
        let [first, server, notes] = ids(&rt);
        let (f, s, n) = (
            rect(&rt, &format!("pane-{first}")),
            rect(&rt, &format!("pane-{server}")),
            rect(&rt, &format!("pane-{notes}")),
        );
        assert!(s[0] > f[0] && (s[0] - n[0]).abs() < 1.0 && n[1] > s[1]);
        assert!(
            (f[3] - (s[3] + n[3])).abs() < 10.0,
            "the column fills the height"
        );
    }

    #[test]
    fn selecting_a_tab_outside_the_split_should_show_it_alone_until_a_split_tab_returns() {
        let mut rt = scene("terminal");
        let [first, server, notes] = ids(&rt);
        rt.state
            .drop_tab(server, panes::DropTarget::Pane(first, panes::Edge::Right));
        rt.state.select(notes);
        rt.invalidate();
        rt.frame(2.0 / 60.0);
        assert!(rt.find(&format!("pane-{first}")).is_none());
        assert!(rt.find(&format!("pane-{notes}")).is_some());
        rt.state.select(first);
        rt.invalidate();
        rt.frame(3.0 / 60.0);
        rt.frame(4.0 / 60.0);
        assert!(rt.find(&format!("pane-{server}")).is_some());
        // Back in its pane, the tab's grid fits the pane again, not the whole window.
        assert!(grids(&rt)[1].0 < grids(&rt)[2].0, "{:?}", grids(&rt));
    }

    #[test]
    fn closing_a_split_tab_should_leave_the_rest_split_or_go_back_to_one() {
        let mut rt = scene("arranged");
        let [first, server, notes] = ids(&rt);
        rt.state.close(notes);
        assert_eq!(
            rt.state.split.as_ref().map(panes::PaneTree::leaves),
            Some(vec![first, server])
        );
        rt.state.close(server);
        assert!(rt.state.split.is_none());
    }

    fn rect(rt: &Runtime<App>, key: &str) -> [f32; 4] {
        rt.presented(key).expect(key).0
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
