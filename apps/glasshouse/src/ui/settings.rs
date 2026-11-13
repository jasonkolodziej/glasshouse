//! Settings as a titled box: Security (Touch ID for sudo, Secure Keyboard Entry), SSH (Secure
//! Enclave keys and Glasshouse's hosts) and Appearance (theme, cursor, window, contrast). Drawn
//! like the rest of the window: monospace, `[x]` checkboxes, `(•)` radio sets, `[ buttons ]`.

use glasshouse_security::TouchIDSudo;
use rune::tok::Family;
use rune::{El, ScrollSpec, TextEdit, col, el, row, text, text_input};
use ssh_config::{ManagedSshHost, SshIncludeStatus};
use terminal_theme::{ContrastLevel, CursorShape};

use super::{Ink, UI, button, hrule, pill, style, titled, vrule};
use crate::app::{App, HostDraft, SettingsPane, SettingsState};

const WIDTH: f32 = 860.0;
const HEIGHT: f32 = 580.0;
const NAV: f32 = 180.0;

pub fn sheet(app: &App, ink: &Ink) -> El {
    let Some(settings) = &app.settings else {
        return el();
    };
    let (w, h) = if app.window.0 > 0.0 {
        app.window
    } else {
        crate::SIZE
    };
    let (sw, sh) = (WIDTH.min(w - 40.0), HEIGHT.min(h - 40.0));
    let content = match settings.pane {
        SettingsPane::Security => security(app, settings, ink),
        SettingsPane::Ssh => ssh(settings, ink),
        SettingsPane::Appearance => appearance(app, settings, ink),
    };
    titled(
        row()
            .key("settings")
            .role("dialog")
            .label("Settings")
            .absolute((w - sw) / 2.0, (h - sh) / 2.0)
            .size(sw, sh),
        "settings",
        ink,
    )
    .child(nav(settings.pane, ink))
    .child(vrule(ink))
    .child(
        col()
            .grow(1.0)
            .min_w(0.0)
            .h_full()
            .child(
                row()
                    .w_full()
                    .h(40.0)
                    .px(18.0)
                    .pt(6.0)
                    .center_y()
                    .child(
                        text(pane_title(settings.pane))
                            .font(style(14.0, 700.0))
                            .color(ink.text),
                    )
                    .child(el().grow(1.0))
                    .child(
                        button("settings-done", "done", ink, true)
                            .on_press(|s: &mut App| s.settings = None),
                    ),
            )
            .child(hrule(ink))
            .child(
                col()
                    .key(format!("settings-scroll-{:?}", settings.pane))
                    .w_full()
                    .min_w(0.0)
                    .grow(1.0)
                    .scroll(ScrollSpec {
                        stick_bottom: false,
                    })
                    .child(col().w_full().padding(18.0).gap(16.0).child(content)),
            ),
    )
}

fn pane_title(pane: SettingsPane) -> &'static str {
    match pane {
        SettingsPane::Security => "security",
        SettingsPane::Ssh => "ssh keys and hosts",
        SettingsPane::Appearance => "appearance",
    }
}

fn nav(current: SettingsPane, ink: &Ink) -> El {
    let item = |pane: SettingsPane, n: &'static str, label: &'static str| {
        let on = pane == current;
        let mut r = row()
            .key(format!("nav-{label}"))
            .role("tab")
            .label(label)
            .hit()
            .w_full()
            .h(26.0)
            .px(10.0)
            .gap(8.0)
            .center_y()
            .hover(ink.hover)
            .on_press(move |s: &mut App| s.open_settings(pane))
            .child(
                text(if on { "▸" } else { " " })
                    .font(style(UI, 700.0))
                    .color(ink.accent),
            )
            .child(text(n).font(style(UI, 600.0)).color(ink.tertiary))
            .child(
                text(label)
                    .font(style(UI, if on { 650.0 } else { 450.0 }))
                    .color(if on { ink.text } else { ink.secondary }),
            );
        if on {
            r = r.fill(ink.selected);
        }
        r
    };
    col()
        .w(NAV)
        .h_full()
        .pt(18.0)
        .gap(2.0)
        .fill(ink.surface(ink.panel))
        .child(item(SettingsPane::Security, "1", "security"))
        .child(item(SettingsPane::Ssh, "2", "ssh"))
        .child(item(SettingsPane::Appearance, "3", "appearance"))
}

// MARK: Building blocks

/// `── title ─────────`, then the section's rows.
fn section(title: &str, ink: &Ink) -> El {
    col().w_full().gap(6.0).child(
        row()
            .w_full()
            .gap(8.0)
            .center_y()
            .child(text("──").font(style(UI, 500.0)).color(ink.hairline))
            .child(
                text(title.to_lowercase())
                    .font(style(UI, 650.0))
                    .color(ink.accent),
            )
            .child(el().grow(1.0).h(1.0).fill(ink.hairline)),
    )
}

/// Rows of a section, indented under its title.
fn group(_ink: &Ink) -> El {
    col().w_full().pl(16.0)
}

fn line(_ink: &Ink) -> El {
    row()
        .w_full()
        .min_w(0.0)
        .min_h(28.0)
        .py(2.0)
        .gap(10.0)
        .center_y()
}

fn note(s: impl Into<String>, ink: &Ink) -> El {
    text(s.into())
        .w_full()
        .wrap()
        .font(style(11.5, 450.0))
        .color(ink.tertiary)
        .pl(16.0)
}

fn label(s: impl Into<String>, ink: &Ink) -> El {
    text(s.into()).font(style(UI, 450.0)).color(ink.text)
}

fn status(ok: bool, s: impl Into<String>, ink: &Ink) -> El {
    row()
        .gap(8.0)
        .center_y()
        .child(
            text(if ok { "✓" } else { "!" })
                .font(style(UI, 700.0))
                .color(if ok { ink.green } else { ink.orange }),
        )
        .child(text(s.into()).font(style(UI, 500.0)).color(ink.text))
}

/// `[x]` / `[ ]`.
fn toggle(key: &str, on: bool, ink: &Ink, set: fn(&mut App, bool)) -> El {
    row()
        .key(key)
        .role("switch")
        .label(key.to_owned())
        .hit()
        .h(24.0)
        .px(2.0)
        .center_y()
        .hover(ink.hover)
        .on_press(move |s: &mut App| set(s, !on))
        .child(text("[").font(style(UI, 500.0)).color(ink.tertiary))
        .child(
            text(if on { "x" } else { " " })
                .font(style(UI, 800.0))
                .color(ink.accent),
        )
        .child(text("]").font(style(UI, 500.0)).color(ink.tertiary))
}

fn toggle_row(title: &str, key: &str, on: bool, ink: &Ink, set: fn(&mut App, bool)) -> El {
    line(ink)
        .child(toggle(key, on, ink, set))
        .child(label(title, ink).grow(1.0).min_w(0.0).wrap())
}

/// `(•) one  ( ) two  ( ) three`.
fn segmented<T: Copy + PartialEq + 'static>(
    key: &str,
    options: &[(T, &'static str)],
    current: T,
    ink: &Ink,
    set: fn(&mut App, T),
) -> El {
    let segments = options.iter().map(|&(value, name)| {
        let on = value == current;
        row()
            .key(format!("{key}-{name}"))
            .role("radio")
            .label(name)
            .hit()
            .h(24.0)
            .px(4.0)
            .gap(6.0)
            .center_y()
            .hover(ink.hover)
            .on_press(move |s: &mut App| set(s, value))
            .child(
                text(if on { "(•)" } else { "( )" })
                    .font(style(UI, 700.0))
                    .color(if on { ink.accent } else { ink.tertiary }),
            )
            .child(
                text(name.to_lowercase())
                    .font(style(UI, if on { 650.0 } else { 450.0 }))
                    .color(if on { ink.text } else { ink.secondary }),
            )
    });
    row().key(key).gap(10.0).children(segments)
}

/// `› value` over an underline.
fn field(
    key: &str,
    edit: &TextEdit,
    placeholder: &str,
    ink: &Ink,
    bind: fn(&mut App) -> &mut TextEdit,
) -> El {
    row()
        .w(240.0)
        .h(26.0)
        .px(6.0)
        .gap(6.0)
        .center_y()
        .fill(ink.surface(ink.panel))
        .border_sides([0.0, 0.0, 1.0, 0.0], ink.rule)
        .child(text("›").font(style(UI, 700.0)).color(ink.accent))
        .child(
            text_input(edit, style(UI, 500.0).lh(18.0))
                .key(key)
                .single_line()
                .grow(1.0)
                .color(ink.text)
                .caret(ink.accent, ink.accent.alpha(0.3))
                .placeholder(placeholder, ink.tertiary)
                .on_edit(bind),
        )
}

// MARK: Security

fn security(app: &App, s: &SettingsState, ink: &Ink) -> El {
    let t = &s.touch_id;
    let enabled = t.pam_tid_enabled;
    let mut touch = group(ink).child(line(ink).child(status(
        enabled,
        if enabled {
            "Touch ID for sudo is on"
        } else {
            "Touch ID for sudo is off"
        },
        ink,
    )));
    if t.pam_reattach_user_writable {
        touch = touch.child(line(ink).child(
            note(
                "pam_reattach is loaded from a user-writable path. Any process running as you could replace it and gain root on your next sudo. Re-run setup to install a root-owned copy.",
                ink,
            )
            .color(ink.orange),
        ));
    }
    touch = touch
        .child(toggle_row(
            "I use tmux or screen (install pam_reattach, root-owned)",
            "reattach",
            s.include_reattach,
            ink,
            |a, on| {
                if let Some(s) = &mut a.settings {
                    s.include_reattach = on;
                }
            },
        ))
        .child(
            row()
                .w_full()
                .px(14.0)
                .py(10.0)
                .gap(8.0)
                .child(
                    button(
                        "touch-id-setup",
                        if enabled {
                            "Run Setup Again…"
                        } else {
                            "Set Up…"
                        },
                        ink,
                        true,
                    )
                    .on_press(|a: &mut App| {
                        let reattach = a.settings.as_ref().is_some_and(|s| s.include_reattach);
                        a.request_visible_script(
                            "Touch ID setup",
                            Ok(TouchIDSudo::setup_script(reattach)),
                        );
                    }),
                )
                .children(enabled.then(|| {
                    button("touch-id-rollback", "Roll Back…", ink, false).on_press(|a: &mut App| {
                        a.request_visible_script(
                            "Touch ID rollback",
                            Ok(TouchIDSudo::ROLLBACK_SCRIPT.to_owned()),
                        );
                    })
                }))
                .child(el().grow(1.0))
                .child(button("touch-id-refresh", "Refresh", ink, false).on_press(
                    |a: &mut App| {
                        if let Some(s) = &mut a.settings {
                            s.touch_id = TouchIDSudo::status();
                        }
                    },
                )),
        );
    let secure = group(ink)
        .child(
            line(ink).child(label("Automatic", ink).grow(1.0)).child(
                text("On while a password prompt has focus")
                    .font(style(12.5, 450.0))
                    .color(ink.secondary),
            ),
        )
        .child(toggle_row(
            "Always on (CIS benchmark)",
            "secure-always",
            app.secure_always,
            ink,
            |a, on| a.set_secure_always(on),
        ));
    col()
        .w_full()
        .gap(22.0)
        .child(section("Touch ID for sudo", ink).child(touch).child(note(
            "Glasshouse never edits /etc itself. Setup opens a tab showing every command before it runs. Password stays the fallback; each Touch ID tap authorizes exactly one command.",
            ink,
        )))
        .child(section("Secure Keyboard Entry", ink).child(secure).child(note(
            "Always-on can disable global hotkeys and interfere with accessibility tools in other apps.",
            ink,
        )))
}

// MARK: SSH

fn ssh(s: &SettingsState, ink: &Ink) -> El {
    let ssh = &s.ssh;
    let mut keys = group(ink);
    for key in &ssh.keys {
        let public = key.public_key.clone();
        keys = keys.child(
            line(ink)
                .child(text("◆").font(style(14.0, 600.0)).color(ink.accent))
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(1.0)
                        .child(
                            text(format!("~/.ssh/{}", key.name))
                                .font(style(13.0, 550.0).family(Family::Mono))
                                .color(ink.text),
                        )
                        .child(
                            text(key.key_type.clone())
                                .font(style(11.0, 450.0))
                                .color(ink.secondary),
                        ),
                )
                .child(
                    button(&format!("copy-{}", key.name), "Copy Public Key", ink, false)
                        .on_press_tx(move |_: &mut App, tx| tx.copy(public.clone())),
                ),
        );
    }
    keys = keys.child(
        row()
            .w_full()
            .px(14.0)
            .py(10.0)
            .gap(8.0)
            .center_y()
            .child(label("New key", ink))
            .child(field(
                "new-key-name",
                &ssh.new_key,
                "glasshouse",
                ink,
                |a| match &mut a.settings {
                    Some(s) => &mut s.ssh.new_key,
                    None => &mut a.scratch,
                },
            ))
            .child(el().grow(1.0))
            .child(
                button(
                    "create-key",
                    "Create Key in New Tab…",
                    ink,
                    ssh.can_create_key(),
                )
                .on_press(|a: &mut App| a.create_secure_enclave_key()),
            ),
    );

    let mut hosts = group(ink);
    hosts = hosts.child(match ssh.include {
        SshIncludeStatus::Active => line(ink).child(status(
            true,
            "~/.ssh/config includes ~/.ssh/glasshouse.conf",
            ink,
        )),
        SshIncludeStatus::Missing => line(ink)
            .child(
                status(
                    false,
                    "~/.ssh/config doesn't include Glasshouse's hosts yet",
                    ink,
                )
                .grow(1.0),
            )
            .child(
                button("install-include", "Add Include", ink, true)
                    .on_press(|a: &mut App| a.install_include()),
            ),
        SshIncludeStatus::Inactive { line: n } => line(ink)
            .child(
                status(
                    false,
                    format!("The Include on line {n} only applies inside a Host block"),
                    ink,
                )
                .grow(1.0),
            )
            .child(
                button("install-include", "Move It to the Top", ink, true)
                    .on_press(|a: &mut App| a.install_include()),
            ),
    });
    for conflict in &ssh.conflicts {
        hosts = hosts.child(line(ink).child(
            note(
                format!(
                    "~/.ssh/config line {} also defines “{}”. Glasshouse's entry is read first, so its host, user, port and key win.",
                    conflict.line, conflict.alias
                ),
                ink,
            )
            .color(ink.orange),
        ));
    }
    for host in ssh.managed.hosts() {
        hosts = hosts.child(host_row(host, ink));
    }
    hosts = hosts.child(
        row()
            .w_full()
            .px(14.0)
            .py(10.0)
            .gap(8.0)
            .child(
                button("add-host", "Add Host…", ink, true).on_press(|a: &mut App| {
                    if let Some(s) = &mut a.settings {
                        let key = s
                            .ssh
                            .keys
                            .first()
                            .map(|k| k.name.clone())
                            .unwrap_or_else(|| "glasshouse".into());
                        s.ssh.editor = Some(HostDraft::new(key));
                    }
                }),
            )
            .child(el().grow(1.0))
            .child(
                button("ssh-refresh", "Refresh", ink, false).on_press(|a: &mut App| {
                    if let Some(s) = &mut a.settings {
                        s.ssh.reload();
                    }
                }),
            ),
    );

    let mut page = col().w_full().gap(22.0);
    if let Some(error) = &ssh.error {
        page = page.child(note(error.clone(), ink).color(ink.red));
    }
    if let Some(draft) = &ssh.editor {
        page = page.child(editor(draft, ink));
    }
    page.child(section("Secure Enclave keys", ink).child(keys).child(note(
        "Apple's sc_auth and /usr/lib/ssh-keychain.dylib: no Glasshouse code in the signing path. Servers need OpenSSH 8.2 or later (sk- keys). Register a second recovery key: Secure Enclave keys can't be backed up.",
        ink,
    )))
    .child(section("Hosts", ink).child(hosts).child(note(
        "Hosts are written to ~/.ssh/glasshouse.conf. Each one signs only with its Secure Enclave key, so every login asks for Touch ID.",
        ink,
    )))
}

fn host_row(host: &ManagedSshHost, ink: &Ink) -> El {
    let alias = host.alias.clone();
    let (install, edit, remove) = (alias.clone(), host.clone(), alias.clone());
    let target = format!(
        "{}{}{} · ~/.ssh/{}",
        host.user
            .as_deref()
            .map(|u| format!("{u}@"))
            .unwrap_or_default(),
        host.host_name,
        host.port.map(|p| format!(":{p}")).unwrap_or_default(),
        host.key_name
    );
    let backup = if host.backup_keys.is_empty() && host.security_key.is_none() {
        pill("No backup key", ink.orange, ink.orange.alpha(0.16))
    } else {
        pill("Backup key", ink.green, ink.green.alpha(0.16))
    };
    line(ink)
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(2.0)
                .child(
                    row()
                        .gap(8.0)
                        .center_y()
                        .child(
                            text(host.alias.clone())
                                .font(style(13.5, 600.0))
                                .color(ink.text),
                        )
                        .child(backup),
                )
                .child(
                    text(target)
                        .font(style(11.5, 450.0))
                        .color(ink.secondary)
                        .truncate(),
                ),
        )
        .child(
            button(&format!("connect-{alias}"), "Connect", ink, false).on_press(
                move |a: &mut App| {
                    a.settings = None;
                    a.open_host(&alias);
                },
            ),
        )
        .child(
            button(&format!("install-{install}"), "Install Keys…", ink, false)
                .on_press(move |a: &mut App| a.install_keys(&install)),
        )
        .child(
            button(&format!("edit-{remove}"), "Edit…", ink, false).on_press(move |a: &mut App| {
                if let Some(s) = &mut a.settings {
                    s.ssh.editor = Some(HostDraft::editing(&edit));
                }
            }),
        )
        .child(
            row()
                .key(format!("remove-{remove}"))
                .role("button")
                .label(format!("Remove {remove} from Glasshouse"))
                .hit()
                .size(28.0, 28.0)
                .center()
                .radius(0.0)
                .hover(ink.red.alpha(0.15))
                .on_press({
                    let alias = remove.clone();
                    move |a: &mut App| a.remove_host(&alias)
                })
                .child(text("×").font(style(15.0, 600.0)).color(ink.red)),
        )
}

fn editor(draft: &HostDraft, ink: &Ink) -> El {
    fn draft_of(a: &mut App) -> &mut HostDraft {
        if a.settings.as_ref().is_some_and(|s| s.ssh.editor.is_some())
            && let Some(draft) = a.settings.as_mut().and_then(|s| s.ssh.editor.as_mut())
        {
            return draft;
        }
        a.scratch_draft
            .get_or_insert_with(|| HostDraft::new(String::new()))
    }
    let title = if draft.original.is_some() {
        "Edit Host"
    } else {
        "Add Host"
    };
    let row_of = |name: &str, f: El| {
        line(ink)
            .child(label(name.to_owned(), ink).w(150.0))
            .child(f)
    };
    let alias = if draft.alias.text.is_empty() {
        "myserver"
    } else {
        draft.alias.text.as_str()
    };
    let mut g = group(ink)
        .child(row_of(
            "Alias",
            field("draft-alias", &draft.alias, "myserver", ink, |a| {
                &mut draft_of(a).alias
            }),
        ))
        .child(row_of(
            "Host name or IP",
            field(
                "draft-host",
                &draft.host_name,
                "myserver.example.com",
                ink,
                |a| &mut draft_of(a).host_name,
            ),
        ))
        .child(row_of(
            "User",
            field("draft-user", &draft.user, "Optional", ink, |a| {
                &mut draft_of(a).user
            }),
        ))
        .child(row_of(
            "Port",
            field("draft-port", &draft.port, "22", ink, |a| {
                &mut draft_of(a).port
            }),
        ))
        .child(row_of(
            "Key",
            text(format!("~/.ssh/{}", draft.key_name))
                .font(style(13.0, 500.0).family(Family::Mono))
                .color(ink.text),
        ))
        .child(line(ink).child(note(
            format!("You type the alias after ssh, as in “ssh {alias}”."),
            ink,
        )));
    if let Some(error) = &draft.error {
        g = g.child(line(ink).child(note(error.clone(), ink).color(ink.red)));
    }
    g = g.child(
        row()
            .w_full()
            .px(14.0)
            .py(10.0)
            .gap(8.0)
            .child(el().grow(1.0))
            .child(
                button("draft-cancel", "Cancel", ink, false).on_press(|a: &mut App| {
                    if let Some(s) = &mut a.settings {
                        s.ssh.editor = None;
                    }
                }),
            )
            .child(
                button("draft-save", "Save", ink, true).on_press(|a: &mut App| a.save_host_draft()),
            ),
    );
    section(title, ink).child(g)
}

// MARK: Appearance

fn appearance(app: &App, s: &SettingsState, ink: &Ink) -> El {
    let config = &app.themes.config;
    let current = app.themes.resolved(app.env.dark);
    let follow = config.theme.follow_system;
    let dark_id = config.theme.dark.clone();
    let light_id = config.theme.light.clone();

    let swatches = app.themes.themes.iter().map(|theme| {
        let c = &theme.colors;
        let bg = super::color(crate::appearance::rgba(c.background));
        let fg = super::color(crate::appearance::rgba(c.foreground));
        let ansi = c.ansi();
        let in_use = theme.id == current.theme.id;
        let id = theme.id.clone();
        let dots = [1usize, 2, 3, 4, 5, 6].map(|i| {
            el().size(12.0, 10.0)
                .radius(0.0)
                .fill(super::color(crate::appearance::rgba(ansi[i])))
        });
        let mut card = col()
            .key(format!("theme-{}", theme.id))
            .role("radio")
            .label(theme.name.clone())
            .hit()
            .w(150.0)
            .gap(6.0)
            .on_press(move |a: &mut App| {
                let dark = a.env.dark || !a.themes.config.theme.follow_system;
                let id = id.clone();
                a.update_appearance(move |c| {
                    if dark {
                        c.theme.dark = id;
                    } else {
                        c.theme.light = id;
                    }
                });
            })
            .child(
                col()
                    .w_full()
                    .h(70.0)
                    .padding(9.0)
                    .gap(6.0)
                    .radius(0.0)
                    .fill(bg)
                    .border(
                        if in_use { 2.0 } else { 0.5 },
                        if in_use { ink.accent } else { ink.hairline },
                    )
                    .child(
                        text("~ % ls")
                            .font(style(11.0, 500.0).family(Family::Mono))
                            .color(fg),
                    )
                    .child(row().gap(4.0).children(dots)),
            )
            .child(
                text(theme.name.clone())
                    .font(style(12.0, if in_use { 600.0 } else { 450.0 }))
                    .color(ink.text)
                    .truncate(),
            );
        if theme.id == dark_id || (follow && theme.id == light_id) {
            card = card.child(
                text(if theme.id == dark_id {
                    "Dark Mode"
                } else {
                    "Light Mode"
                })
                .font(style(10.5, 500.0))
                .color(ink.secondary),
            );
        }
        card
    });

    let opacity = config.window.opacity.unwrap_or(current.opacity);
    let opacity_steps: [(u8, &'static str); 5] = [
        (55, "55%"),
        (70, "70%"),
        (80, "80%"),
        (90, "90%"),
        (100, "100%"),
    ];
    let opacity_now = opacity_steps
        .iter()
        .min_by_key(|(p, _)| (f64::from(*p) - opacity * 100.0).abs() as i64)
        .map_or(100, |(p, _)| *p);

    let mut page = col()
        .w_full()
        .gap(22.0)
        .child(section("Theme", ink).child(group(ink).child(toggle_row(
            "Match the system's Light and Dark Mode",
            "follow-system",
            follow,
            ink,
            |a, on| a.update_appearance(|c| c.theme.follow_system = on),
        ))))
        .child(section("Gallery", ink).child(row().w_full().flex_wrap().gap(14.0).children(swatches)))
        .child(section("Cursor", ink).child(
            group(ink)
                .child(line(ink).child(label("Shape", ink).grow(1.0)).child(segmented(
                    "cursor-shape",
                    &[(CursorShape::Block, "Block"), (CursorShape::Underline, "Underline"), (CursorShape::Bar, "Bar")],
                    current.cursor_shape,
                    ink,
                    |a, v| a.update_appearance(|c| c.cursor.style = Some(v)),
                )))
                .child(toggle_row("Blink", "cursor-blink", current.cursor_blink, ink, |a, on| {
                    a.update_appearance(|c| c.cursor.blink = Some(on))
                })),
        ))
        .child(section("Window", ink).child(group(ink).child(line(ink).child(label("Background opacity", ink).grow(1.0)).child(
            segmented("opacity", &opacity_steps, opacity_now, ink, |a, v| {
                a.update_appearance(|c| c.window.opacity = Some(f64::from(v) / 100.0))
            }),
        ))).child(note("Below 100% the pane is frosted glass: what's behind it blurs through. Increase Contrast and Reduce Transparency make it opaque.", ink)))
        .child(section("Contrast", ink).child(group(ink).child(line(ink).child(label("Minimum contrast", ink).grow(1.0)).child(
            segmented(
                "contrast",
                &[(ContrastLevel::Standard, "Standard"), (ContrastLevel::Faithful, "Faithful"), (ContrastLevel::High, "High")],
                config.contrast,
                ink,
                |a, v| a.update_appearance(|c| c.contrast = v),
            ),
        ))).child(note(
            "Glasshouse nudges colours that are too close to the background (APCA): text to at least Lc 75 and ANSI colours to at least Lc 60. Faithful lowers the ANSI floor to Lc 45; High raises both.",
            ink,
        )));
    if let Some(error) = &s.appearance_error {
        page = page.child(note(error.clone(), ink).color(ink.red));
    }
    for problem in &app.themes.problems {
        page = page
            .child(note(format!("{}: {}", problem.file, problem.problem), ink).color(ink.orange));
    }
    page
}
