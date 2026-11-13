//! Scripted tabs for snapshot scenes and `--demo`: real engines fed with recorded output (colours,
//! OSC 133 marks, OSC 7 directories), so the grid, gutter and palette show what a live shell would.

use glasshouse_security::TouchIDSudoStatus;
use rune::TextEdit;
use ssh_config::{ManagedSshConfig, ManagedSshHost, SshIncludeStatus, SshKeyFile};
use terminal_core::GridSize;

use crate::app::{
    App, HostDraft, PaletteState, SettingsPane, SettingsState, SshSettings, SystemEnv,
};
use crate::appearance::ThemeLibrary;
use crate::hosts::{Credential, SshHost};
use crate::session::{ApprovalContext, ApprovalPhase, ApprovalSource, Session};

pub type Scene = (&'static str, fn(&mut App));

pub const SCENES: &[Scene] = &[
    ("terminal", terminal),
    ("sidebar", sidebar),
    ("palette", palette),
    ("approval", approval),
    ("find", find),
    ("settings", settings_security),
    ("ssh", settings_ssh),
    ("appearance", settings_appearance),
    ("light", light),
    ("glass", glass),
];

const GRID: GridSize = GridSize {
    cols: 132,
    rows: 38,
};

const ESC: &str = "\x1b";

fn prompt(dir: &str) -> String {
    format!(
        "{ESC}]7;file://studio{dir_path}\x07{ESC}]133;A\x07{ESC}[1;38;5;114myou{ESC}[0m{ESC}[2m@{ESC}[0m{ESC}[38;5;110mstudio{ESC}[0m {ESC}[1;38;5;75m{dir}{ESC}[0m {ESC}[38;5;176m❯{ESC}[0m {ESC}]133;B\x07",
        dir_path = dir.replacen(
            '~',
            &std::env::var("HOME").unwrap_or_else(|_| "/home/you".into()),
            1
        ),
    )
}

/// One finished command: prompt, the typed line, its output and exit status.
fn command(dir: &str, line: &str, output: &str, code: i32) -> String {
    format!(
        "{}{line}\n{ESC}]133;C\x07{output}{ESC}]133;D;{code}\x07",
        prompt(dir)
    )
}

fn main_transcript() -> String {
    let mut t = String::new();
    t += &command(
        "~/src/glasshouse",
        "git log --oneline -6",
        &format!(
            "{y}f402b4d{r} {c}({b}HEAD -> {g}main{c}){r} feat(rust): port Glasshouse libraries and agent\n\
             {y}889ab81{r} Themes: YAML theme files, 15 templates, a gallery and a theme editor\n\
             {y}b464f16{r} Backup keys, a security key fallback and a server key check\n\
             {y}0485b58{r} Manage Touch ID SSH hosts from Settings\n\
             {y}c54ca7a{r} Document Touch ID and SSH setup with auth flow diagrams\n\
             {y}aacc695{r} Wire the app icon catalog into the Glasshouse target\n",
            y = "\x1b[33m",
            r = "\x1b[0m",
            c = "\x1b[33m",
            b = "\x1b[1;36m",
            g = "\x1b[1;32m"
        ),
        0,
    );
    t += &command(
        "~/src/glasshouse",
        "ls",
        "\x1b[1;34mGlasshouse\x1b[0m  \x1b[1;34mapps\x1b[0m  \x1b[1;34mcrates\x1b[0m  \x1b[1;34mdocs\x1b[0m  Cargo.lock  Cargo.toml  LICENSE  README.md  rust-toolchain.toml\n",
        0,
    );
    t += &command(
        "~/src/glasshouse",
        "cargo test --workspace -q",
        "\n\x1b[1mrunning 214 tests\x1b[0m\n\
         ........................................................................................ 88/214\n\
         ........................................................................................ 176/214\n\
         ......................................\n\
         test result: \x1b[1;32mok\x1b[0m. 214 passed; 0 failed; 0 ignored; finished in 1.84s\n\n",
        0,
    );
    t += &command(
        "~/src/glasshouse",
        "cargo clippy -p ssh-config",
        "\x1b[1;33mwarning\x1b[0m\x1b[1m: unused variable: `port`\x1b[0m\n  \x1b[1;34m-->\x1b[0m crates/ssh-config/src/managed_ssh_host.rs:88:13\n\x1b[1;31merror\x1b[0m\x1b[1m: could not compile `ssh-config` due to 1 previous error\x1b[0m\n",
        101,
    );
    t += &command(
        "~/src/glasshouse",
        "git status -sb",
        "## \x1b[32mclaude/ecstatic-ride\x1b[0m...\x1b[31morigin/claude/ecstatic-ride\x1b[0m [ahead 2]\n \x1b[31mM\x1b[0m apps/glasshouse/src/main.rs\n\x1b[32mA\x1b[0m  apps/glasshouse/src/ui/grid.rs\n\x1b[32mA\x1b[0m  apps/glasshouse/src/ui/mod.rs\n",
        0,
    );
    t += &prompt("~/src/glasshouse");
    t
}

fn server_transcript() -> String {
    let mut t = String::new();
    t += &command(
        "~",
        "ssh myserver uptime",
        " 14:02:11 up 41 days,  3:12,  2 users,  load average: 0.08, 0.11, 0.09\n",
        0,
    );
    t += &command("~", "htop --version", "htop 3.3.0\n", 0);
    t += &prompt("~");
    t
}

fn sudo_transcript() -> String {
    let mut t = String::new();
    t += &command(
        "~/src/glasshouse",
        "brew services list",
        "\x1b[1mName       Status  User  File\x1b[0m\npostgresql \x1b[32mstarted\x1b[0m you ~/Library/LaunchAgents/homebrew.mxcl.postgresql.plist\nredis      \x1b[2mnone\x1b[0m\n",
        0,
    );
    t += &prompt("~/src/glasshouse");
    t += "sudo launchctl kickstart -k system/com.openssh.sshd\n";
    t += &format!("{ESC}]133;C\x07");
    t
}

fn appearance(app: &App) -> terminal_core::TerminalAppearance {
    app.current_appearance()
}

fn add(app: &mut App, title: &str, transcript: &str) -> u64 {
    let a = appearance(app);
    app.add_demo(|id| Session::demo(id, title, a, GRID, transcript))
}

/// Built-in themes, no disk, three tabs: what every scene starts from.
pub fn terminal(app: &mut App) {
    app.themes = ThemeLibrary::built_in();
    app.themes.config.window.opacity = None;
    app.themes.generation += 1;
    let first = add(app, "glasshouse — cargo", &main_transcript());
    let server = add(app, "myserver", &server_transcript());
    if let Some(s) = app.sessions.iter_mut().find(|s| s.id == server) {
        s.trust = design_system::TrustState::RemoteSession("myserver.example.com".into());
    }
    add(
        app,
        "notes",
        &command(
            "~/notes",
            "cat todo.md | head -3",
            "- [x] port the crates\n- [x] build the Rune UI\n- [ ] ship it\n",
            0,
        ),
    );
    app.select(first);
    app.hosts = vec![
        host(
            "myserver",
            Some("you"),
            "myserver.example.com",
            Credential::SecureEnclave,
        ),
        host("build-box", Some("ci"), "10.0.4.12", Credential::Agent),
        host("pi", None, "raspberrypi.local", Credential::Other),
    ];
}

fn host(alias: &str, user: Option<&str>, host_name: &str, credential: Credential) -> SshHost {
    SshHost {
        alias: alias.into(),
        host_name: Some(host_name.into()),
        user: user.map(Into::into),
        port: None,
        credential,
    }
}

fn sidebar(app: &mut App) {
    terminal(app);
    app.show_sidebar = true;
}

fn palette(app: &mut App) {
    terminal(app);
    app.palette = Some(PaletteState {
        edit: TextEdit::new(""),
        selection: 0,
    });
}

fn find(app: &mut App) {
    terminal(app);
    app.find = Some(TextEdit::new("cargo"));
}

fn approval(app: &mut App) {
    terminal(app);
    let a = appearance(app);
    let id = app.add_demo(|id| Session::demo(id, "sshd", a, GRID, &sudo_transcript()));
    if let Some(session) = app.sessions.iter_mut().find(|s| s.id == id) {
        session.cwd = Some(format!(
            "{}/src/glasshouse",
            std::env::var("HOME").unwrap_or_default()
        ));
        session.show_approval(ApprovalContext {
            command: "launchctl kickstart -k system/com.openssh.sshd".into(),
            run_as: "root".into(),
            host: "studio".into(),
            tab_title: "sshd".into(),
            directory: Some("~/src/glasshouse".into()),
            source: ApprovalSource::Kernel,
            requester_pid: Some(48213),
            phase: ApprovalPhase::WaitingForTouchId,
        });
    }
    app.select(id);
}

fn demo_settings(pane: SettingsPane) -> SettingsState {
    let mut managed = ManagedSshConfig::new();
    let mut myserver = ManagedSshHost::new(
        "myserver",
        "myserver.example.com",
        Some("you"),
        None,
        "glasshouse",
    );
    myserver.backup_keys =
        vec!["ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGRlbW8ga2V5IGZvciBzbmFwc2hvdHMgb25seSAxMjM0NTY3 laptop".into()];
    if let Err(e) = managed.upsert(&myserver, None) {
        eprintln!("demo host: {e}");
    }
    let _ = managed.upsert(
        &ManagedSshHost::new(
            "nas",
            "192.168.1.20",
            Some("admin"),
            Some(2222),
            "glasshouse",
        ),
        None,
    );
    SettingsState {
        pane,
        touch_id: TouchIDSudoStatus {
            pam_tid_enabled: true,
            pam_reattach_configured: false,
            pam_reattach_user_writable: false,
            sudo_local_exists: true,
        },
        include_reattach: false,
        ssh: SshSettings {
            store: None,
            keys: vec![SshKeyFile {
                name: "glasshouse".into(),
                key_type: "sk-ecdsa-sha2-nistp256@openssh.com".into(),
                public_key: "sk-ecdsa-sha2-nistp256@openssh.com AAAAInNrLWVjZHNh… you@studio"
                    .into(),
                comment: "you@studio".into(),
            }],
            managed,
            include: SshIncludeStatus::Active,
            conflicts: Vec::new(),
            new_key: TextEdit::new("glasshouse-2"),
            editor: None,
            error: None,
        },
        appearance_error: None,
    }
}

fn settings_security(app: &mut App) {
    terminal(app);
    app.settings = Some(demo_settings(SettingsPane::Security));
}

fn settings_ssh(app: &mut App) {
    terminal(app);
    let mut settings = demo_settings(SettingsPane::Ssh);
    if settings.ssh.keys.is_empty() {
        settings.ssh.editor = Some(HostDraft::new("glasshouse".into()));
    }
    app.settings = Some(settings);
}

fn settings_appearance(app: &mut App) {
    terminal(app);
    app.settings = Some(demo_settings(SettingsPane::Appearance));
}

fn light(app: &mut App) {
    app.set_env(SystemEnv {
        dark: false,
        increase_contrast: false,
    });
    sidebar(app);
}

/// The translucent variant: below 100% opacity the wallpaper and blur show through.
fn glass(app: &mut App) {
    sidebar(app);
    app.themes.config.window.opacity = Some(0.72);
    app.themes.generation += 1;
    let appearance = app.current_appearance();
    let generation = app.themes.generation;
    for session in &mut app.sessions {
        session.apply_appearance(appearance.clone(), generation);
    }
}
