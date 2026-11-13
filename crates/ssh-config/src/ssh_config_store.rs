use std::collections::HashSet;
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};
use std::path::{Component, Path, PathBuf};

use crate::swift_text::{first_line, split_max};
use crate::{
    ManagedSshConfig, ManagedSshHost, SshConfigConflict, SshConfigError, SshConfigStoreError,
    SshIncludeStatus, SshPublicKey, glasshouse_ssh, ssh_config_text,
};

/// Maximum `Include` nesting followed by [`SshConfigStore::host_aliases`].
const MAX_INCLUDE_DEPTH: usize = 16;

/// A security key handle in `~/.ssh`: `<name>` plus `<name>.pub` of an `sk-` type. Secure Enclave keys are
/// `sk-ecdsa-sha2-nistp256@openssh.com`; a FIDO security key may be that type or `sk-ssh-ed25519@openssh.com`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SshKeyFile {
    /// File name under `~/.ssh` of the handle.
    pub name: String,
    /// Key type from the `.pub` file.
    pub key_type: String,
    /// The whole `.pub` line, ready to paste into `authorized_keys`.
    pub public_key: String,
    /// The comment from the `.pub` line, possibly empty.
    pub comment: String,
}

impl SshKeyFile {
    /// Stable identity: the name.
    pub fn id(&self) -> &str {
        &self.name
    }
}

/// Reads and writes Glasshouse's SSH files:
/// - `store_path`: the managed hosts as JSON, the source of truth;
/// - `~/.ssh/glasshouse.conf`: generated from that JSON;
/// - `~/.ssh/config`: changed only by [`install_include`](Self::install_include), which adds the managed block
///   at the top.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshConfigStore {
    ssh_directory: PathBuf,
    store_path: PathBuf,
    home_directory: PathBuf,
}

impl SshConfigStore {
    /// A store over `ssh_directory`, keeping the host list at `store_path`. `home_directory` expands `~` in
    /// `Include` lines; it defaults to the parent of `ssh_directory`.
    pub fn new(ssh_directory: &Path, store_path: &Path, home_directory: Option<&Path>) -> Self {
        let home_directory = home_directory.map(Path::to_path_buf).unwrap_or_else(|| {
            ssh_directory
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default()
        });
        Self {
            ssh_directory: ssh_directory.to_path_buf(),
            store_path: store_path.to_path_buf(),
            home_directory,
        }
    }

    /// `~/.ssh`, and the host list in Application Support (`$XDG_DATA_HOME` or `~/.local/share` elsewhere).
    pub fn standard() -> Result<Self, SshConfigStoreError> {
        let home = std::env::home_dir().ok_or(SshConfigStoreError::NoHomeDirectory)?;
        Ok(Self::new(
            &home.join(".ssh"),
            &application_support(&home).join("Glasshouse/ssh-hosts.json"),
            Some(&home),
        ))
    }

    /// The `~/.ssh` directory.
    pub fn ssh_directory(&self) -> &Path {
        &self.ssh_directory
    }

    /// Where the host list is kept as JSON.
    pub fn store_path(&self) -> &Path {
        &self.store_path
    }

    /// What `~` means in `Include` lines.
    pub fn home_directory(&self) -> &Path {
        &self.home_directory
    }

    /// `~/.ssh/config`.
    pub fn main_config_path(&self) -> PathBuf {
        self.ssh_directory.join("config")
    }

    /// `~/.ssh/glasshouse.conf`.
    pub fn include_file_path(&self) -> PathBuf {
        self.ssh_directory.join(glasshouse_ssh::INCLUDE_FILE_NAME)
    }

    /// The copy of `~/.ssh/config` taken before Glasshouse first changed it.
    pub fn backup_path(&self) -> PathBuf {
        self.ssh_directory.join("config.glasshouse-backup")
    }

    // Managed hosts

    /// The saved hosts; none if the file can't be read.
    pub fn load(&self) -> Result<ManagedSshConfig, SshConfigStoreError> {
        match fs::read(&self.store_path) {
            Ok(data) => ManagedSshConfig::from_json(&data),
            Err(_) => Ok(ManagedSshConfig::new()),
        }
    }

    /// Saves the host list, then regenerates `~/.ssh/glasshouse.conf` from it.
    pub fn save(&self, config: &ManagedSshConfig) -> Result<(), SshConfigStoreError> {
        write(config.to_json()?.as_bytes(), &self.store_path, 0o600, 0o700)?;
        write(
            config.render_include_file().as_bytes(),
            &self.include_file_path(),
            0o600,
            0o700,
        )
    }

    // ~/.ssh/config

    /// Whether `~/.ssh/config` includes the managed file where it takes effect.
    pub fn include_status(&self) -> SshIncludeStatus {
        ssh_config_text::include_status(&self.main_config_text())
    }

    /// Adds the managed block at the top of `~/.ssh/config`, or moves it there. Creates the file if needed.
    /// The first time it changes an existing file, it keeps a copy as `config.glasshouse-backup`.
    pub fn install_include(&self) -> Result<(), SshConfigStoreError> {
        let main = self.main_config_path();
        // Keep a dotfiles symlink pointing where it did.
        let target = fs::canonicalize(&main).unwrap_or(main);
        let original = self.main_config_text();
        let updated = ssh_config_text::installing_include(&original);
        if updated == original {
            return Ok(());
        }
        let backup = self.backup_path();
        if target.exists() && !backup.exists() {
            fs::copy(&target, &backup).map_err(SshConfigStoreError::io(&backup))?;
        }
        let permissions = fs::metadata(&target)
            .map(|m| m.permissions().mode() & 0o7777)
            .unwrap_or(0o600);
        write(updated.as_bytes(), &target, permissions, 0o700)
    }

    /// `Host` blocks in `~/.ssh/config` that also name one of `config`'s aliases.
    pub fn conflicts(&self, config: &ManagedSshConfig) -> Vec<SshConfigConflict> {
        let aliases: Vec<String> = config
            .hosts()
            .iter()
            .flat_map(ManagedSshHost::aliases)
            .collect();
        ssh_config_text::conflicts(&self.main_config_text(), &aliases)
    }

    fn main_config_text(&self) -> String {
        fs::read_to_string(self.main_config_path()).unwrap_or_default()
    }

    // Keys

    /// Secure Enclave key handles in `~/.ssh`, by name. A `.pub` without its handle file is skipped.
    pub fn secure_enclave_keys(&self) -> Vec<SshKeyFile> {
        self.security_key_files()
            .into_iter()
            .filter(|key| key.key_type == glasshouse_ssh::SECURE_ENCLAVE_ALGORITHM)
            .collect()
    }

    /// Every `sk-` key handle in `~/.ssh`, by name: Secure Enclave keys and FIDO security keys alike.
    pub fn security_key_files(&self) -> Vec<SshKeyFile> {
        let mut names: Vec<String> = fs::read_dir(&self.ssh_directory)
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        names.retain(|name| name.ends_with(".pub"));
        names.sort();
        names
            .iter()
            .filter_map(|pub_name| self.security_key_file(pub_name))
            .collect()
    }

    fn security_key_file(&self, pub_name: &str) -> Option<SshKeyFile> {
        let name = pub_name.strip_suffix(".pub")?;
        if !self.ssh_directory.join(name).exists() {
            return None;
        }
        let text = fs::read_to_string(self.ssh_directory.join(pub_name)).ok()?;
        let line = first_line(&text)?;
        let fields = split_max(line, 2, |c| c == ' ');
        if fields.len() < 2 || !glasshouse_ssh::SECURITY_KEY_ALGORITHMS.contains(&fields[0]) {
            return None;
        }
        Some(SshKeyFile {
            name: name.to_owned(),
            key_type: fields[0].to_owned(),
            public_key: line.to_owned(),
            comment: fields.get(2).copied().unwrap_or("").to_owned(),
        })
    }

    /// The parsed public half of `~/.ssh/<name>`.
    pub fn public_key(&self, name: &str) -> Result<SshPublicKey, SshConfigError> {
        let missing = || SshConfigError::MissingPublicKey(name.to_owned());
        if !ManagedSshHost::is_valid_key_name(name) {
            return Err(missing());
        }
        let text = fs::read_to_string(self.ssh_directory.join(format!("{name}.pub")))
            .map_err(|_| missing())?;
        let line = first_line(&text).ok_or_else(missing)?;
        SshPublicKey::parse(line).map_err(|_| missing())
    }

    // Host aliases across Include files

    /// Concrete host aliases from `~/.ssh/config` and every file it includes, the way ssh reads them:
    /// relative `Include` paths start in `~/.ssh`, and `*` or `?` may appear in the last path component.
    pub fn host_aliases(&self) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut visited = HashSet::new();
        let mut result = Vec::new();
        self.visit(
            &self.main_config_path(),
            0,
            &mut seen,
            &mut visited,
            &mut result,
        );
        result
    }

    fn visit(
        &self,
        path: &Path,
        depth: usize,
        seen: &mut HashSet<String>,
        visited: &mut HashSet<PathBuf>,
        result: &mut Vec<String>,
    ) {
        if depth > MAX_INCLUDE_DEPTH || !visited.insert(normalize(path)) {
            return;
        }
        let Ok(text) = fs::read_to_string(path) else {
            return;
        };
        for alias in ssh_config_text::host_aliases(&text) {
            if seen.insert(alias.clone()) {
                result.push(alias);
            }
        }
        for pattern in ssh_config_text::include_patterns(&text) {
            for file in self.expand_include(&pattern) {
                self.visit(&file, depth + 1, seen, visited, result);
            }
        }
    }

    fn expand_include(&self, pattern: &str) -> Vec<PathBuf> {
        let path = if let Some(rest) = pattern.strip_prefix("~/") {
            self.home_directory.join(rest)
        } else if pattern.starts_with('/') {
            PathBuf::from(pattern)
        } else {
            self.ssh_directory.join(pattern)
        };
        let Some(last) = path.file_name().and_then(|name| name.to_str()) else {
            return vec![path];
        };
        if !last.contains('*') && !last.contains('?') {
            return vec![path];
        }
        let directory = path.parent().unwrap_or(Path::new("/"));
        let mut entries: Vec<String> = fs::read_dir(directory)
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        entries.sort();
        let pattern: Vec<char> = last.chars().collect();
        entries
            .into_iter()
            .filter(|entry| fnmatch(&pattern, &entry.chars().collect::<Vec<_>>()))
            .map(|entry| directory.join(entry))
            .collect()
    }
}

#[cfg(target_os = "macos")]
fn application_support(home: &Path) -> PathBuf {
    home.join("Library/Application Support")
}

#[cfg(not(target_os = "macos"))]
fn application_support(home: &Path) -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(".local/share"))
}

/// Removes `.` and `..` without touching the disk, like `URL.standardizedFileURL`.
fn normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other),
        }
    }
    result
}

/// `fnmatch(pattern, name, 0)`: `*`, `?`, `[…]` (with `!` or `^` to negate) and `\` escapes. With no flags,
/// wildcards match `/` and a leading `.`.
fn fnmatch(pattern: &[char], name: &[char]) -> bool {
    let (mut p, mut n) = (0, 0);
    let mut backtrack: Option<(usize, usize)> = None;
    while n < name.len() {
        if p < pattern.len() && pattern[p] == '*' {
            p += 1;
            backtrack = Some((p, n));
            continue;
        }
        if let Some(next) = match_one(pattern, p, name[n]) {
            p = next;
            n += 1;
            continue;
        }
        match backtrack {
            Some((star_p, star_n)) => {
                p = star_p;
                n = star_n + 1;
                backtrack = Some((star_p, star_n + 1));
            }
            None => return false,
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

/// If the pattern element at `p` matches `c`, the index after that element.
fn match_one(pattern: &[char], p: usize, c: char) -> Option<usize> {
    match *pattern.get(p)? {
        '?' => Some(p + 1),
        '[' => match match_bracket(pattern, p, c) {
            Some((true, next)) => Some(next),
            Some((false, _)) => None,
            None => (c == '[').then_some(p + 1),
        },
        '\\' if p + 1 < pattern.len() => (pattern[p + 1] == c).then_some(p + 2),
        literal => (literal == c).then_some(p + 1),
    }
}

/// Whether the bracket expression starting at `p` matches `c`, and the index after it; `None` if unterminated.
fn match_bracket(pattern: &[char], p: usize, c: char) -> Option<(bool, usize)> {
    let mut i = p + 1;
    let negate = matches!(pattern.get(i), Some('!' | '^'));
    if negate {
        i += 1;
    }
    let mut matched = false;
    let mut first = true;
    loop {
        let mut low = *pattern.get(i)?;
        if low == ']' && !first {
            return Some((matched != negate, i + 1));
        }
        first = false;
        if low == '\\' {
            i += 1;
            low = *pattern.get(i)?;
        }
        i += 1;
        let mut high = low;
        if pattern.get(i) == Some(&'-') && pattern.get(i + 1).is_some_and(|&next| next != ']') {
            high = pattern[i + 1];
            i += 2;
            if high == '\\' {
                high = *pattern.get(i)?;
                i += 1;
            }
        }
        if low <= c && c <= high {
            matched = true;
        }
    }
}

/// Atomic write, then `permissions`. Creates missing parent directories with `directory_permissions`.
fn write(
    data: &[u8],
    path: &Path,
    permissions: u32,
    directory_permissions: u32,
) -> Result<(), SshConfigStoreError> {
    let directory = path.parent().unwrap_or(Path::new("."));
    if !directory.exists() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(directory_permissions)
            .create(directory)
            .map_err(SshConfigStoreError::io(directory))?;
    }
    let mut file =
        tempfile::NamedTempFile::new_in(directory).map_err(SshConfigStoreError::io(directory))?;
    file.write_all(data)
        .map_err(SshConfigStoreError::io(file.path()))?;
    file.persist(path)
        .map_err(|error| SshConfigStoreError::io(path)(error.error))?;
    fs::set_permissions(path, fs::Permissions::from_mode(permissions))
        .map_err(SshConfigStoreError::io(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh_config_text::INCLUDE_BLOCK;
    use crate::test_support::{FIXTURE, key_line, myserver};

    /// A throwaway home with `.ssh` and an Application Support stand-in.
    struct Sandbox {
        home: tempfile::TempDir,
        store: SshConfigStore,
    }

    impl Sandbox {
        fn new() -> Self {
            let home = tempfile::tempdir().unwrap();
            fs::create_dir_all(home.path().join(".ssh")).unwrap();
            let store = SshConfigStore::new(
                &home.path().join(".ssh"),
                &home.path().join("Support/Glasshouse/ssh-hosts.json"),
                Some(home.path()),
            );
            Self { home, store }
        }

        fn path(&self, path: &str) -> PathBuf {
            self.home.path().join(path)
        }

        fn write(&self, text: &str, path: &str) {
            let url = self.path(path);
            fs::create_dir_all(url.parent().unwrap()).unwrap();
            fs::write(url, text).unwrap();
        }

        fn read(&self, path: &str) -> String {
            fs::read_to_string(self.path(path)).unwrap()
        }

        fn permissions(&self, path: &str) -> u32 {
            fs::metadata(self.path(path)).unwrap().permissions().mode() & 0o7777
        }
    }

    #[test]
    fn saves_the_host_list_and_regenerates_the_include_file() {
        let sandbox = Sandbox::new();
        assert_eq!(sandbox.store.load().unwrap(), ManagedSshConfig::new());

        let config = ManagedSshConfig::with_hosts([myserver()]).unwrap();
        sandbox.store.save(&config).unwrap();
        assert_eq!(sandbox.store.load().unwrap(), config);
        assert_eq!(
            sandbox.read(".ssh/glasshouse.conf"),
            config.render_include_file()
        );
        assert_eq!(sandbox.permissions(".ssh/glasshouse.conf"), 0o600);
        assert_eq!(
            sandbox.permissions("Support/Glasshouse/ssh-hosts.json"),
            0o600
        );
        assert_eq!(sandbox.permissions("Support/Glasshouse"), 0o700);
    }

    #[test]
    fn creates_a_missing_config_with_just_the_include() {
        let sandbox = Sandbox::new();
        assert_eq!(sandbox.store.include_status(), SshIncludeStatus::Missing);
        sandbox.store.install_include().unwrap();
        assert_eq!(sandbox.read(".ssh/config"), INCLUDE_BLOCK);
        assert_eq!(sandbox.permissions(".ssh/config"), 0o600);
        assert!(!sandbox.store.backup_path().exists());
        assert_eq!(sandbox.store.include_status(), SshIncludeStatus::Active);
    }

    #[test]
    fn backs_up_an_existing_config_once_and_keeps_its_permissions() {
        let sandbox = Sandbox::new();
        sandbox.write(FIXTURE, ".ssh/config");
        fs::set_permissions(
            sandbox.store.main_config_path(),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();

        sandbox.store.install_include().unwrap();
        assert_eq!(
            sandbox.read(".ssh/config"),
            ssh_config_text::installing_include(FIXTURE)
        );
        assert_eq!(sandbox.read(".ssh/config.glasshouse-backup"), FIXTURE);
        assert_eq!(sandbox.permissions(".ssh/config"), 0o644);

        sandbox.write("# changed later\n", ".ssh/config");
        sandbox.store.install_include().unwrap();
        assert_eq!(sandbox.read(".ssh/config.glasshouse-backup"), FIXTURE);
    }

    #[test]
    fn writes_through_a_symlinked_config() {
        let sandbox = Sandbox::new();
        sandbox.write("Host a\n", "dotfiles/ssh_config");
        std::os::unix::fs::symlink(
            sandbox.path("dotfiles/ssh_config"),
            sandbox.store.main_config_path(),
        )
        .unwrap();
        sandbox.store.install_include().unwrap();
        assert!(
            fs::read_link(sandbox.store.main_config_path())
                .unwrap()
                .ends_with("dotfiles/ssh_config")
        );
        assert_eq!(
            sandbox.read("dotfiles/ssh_config"),
            format!("{INCLUDE_BLOCK}\nHost a\n")
        );
    }

    #[test]
    fn finds_only_secure_enclave_key_handles() {
        let sandbox = Sandbox::new();
        sandbox.write("handle", ".ssh/glasshouse");
        sandbox.write(
            "sk-ecdsa-sha2-nistp256@openssh.com AAAAInNr you@mac\n",
            ".ssh/glasshouse.pub",
        );
        sandbox.write("key", ".ssh/id_ed25519");
        sandbox.write("ssh-ed25519 AAAAC3Nz you@mac\n", ".ssh/id_ed25519.pub");
        sandbox.write(
            "sk-ecdsa-sha2-nistp256@openssh.com AAAAInNr orphan\n",
            ".ssh/orphan.pub",
        );

        let keys = sandbox.store.secure_enclave_keys();
        assert_eq!(
            keys.iter().map(|k| k.name.as_str()).collect::<Vec<_>>(),
            ["glasshouse"]
        );
        assert_eq!(keys[0].comment, "you@mac");
        assert_eq!(
            keys[0].public_key,
            "sk-ecdsa-sha2-nistp256@openssh.com AAAAInNr you@mac"
        );
    }

    #[test]
    fn finds_security_keys_and_reads_public_halves() {
        let sandbox = Sandbox::new();
        sandbox.write("handle", ".ssh/glasshouse");
        sandbox.write(
            &(key_line(glasshouse_ssh::SECURE_ENCLAVE_ALGORITHM, "you@mac", 1) + "\n"),
            ".ssh/glasshouse.pub",
        );
        sandbox.write("handle", ".ssh/id_ed25519_sk");
        sandbox.write(
            &(key_line("sk-ssh-ed25519@openssh.com", "yubikey", 3) + "\n"),
            ".ssh/id_ed25519_sk.pub",
        );

        let files = sandbox.store.security_key_files();
        assert_eq!(
            files.iter().map(|k| k.name.as_str()).collect::<Vec<_>>(),
            ["glasshouse", "id_ed25519_sk"]
        );
        assert_eq!(
            files
                .iter()
                .map(|k| k.key_type.as_str())
                .collect::<Vec<_>>(),
            [
                glasshouse_ssh::SECURE_ENCLAVE_ALGORITHM,
                "sk-ssh-ed25519@openssh.com"
            ]
        );
        let secure_enclave = sandbox.store.secure_enclave_keys();
        assert_eq!(
            secure_enclave
                .iter()
                .map(|k| k.name.as_str())
                .collect::<Vec<_>>(),
            ["glasshouse"]
        );
        assert_eq!(
            sandbox.store.public_key("id_ed25519_sk").unwrap().comment(),
            "yubikey"
        );
        assert_eq!(
            sandbox.store.public_key("nope"),
            Err(SshConfigError::MissingPublicKey("nope".into()))
        );
        assert_eq!(
            sandbox.store.public_key("../x"),
            Err(SshConfigError::MissingPublicKey("../x".into()))
        );
    }

    #[test]
    fn follows_includes_for_host_aliases() {
        let sandbox = Sandbox::new();
        sandbox.write(
            &format!("{INCLUDE_BLOCK}\nInclude config.d/*.conf\nInclude ~/.ssh/config\nHost top\n"),
            ".ssh/config",
        );
        sandbox
            .store
            .save(&ManagedSshConfig::with_hosts([myserver()]).unwrap())
            .unwrap();
        sandbox.write("Host b2\n", ".ssh/config.d/b.conf");
        sandbox.write("Host a1 *.wild\n", ".ssh/config.d/a.conf");
        sandbox.write("Host skipped\n", ".ssh/config.d/notes.txt");

        assert_eq!(
            sandbox.store.host_aliases(),
            ["top", "myserver", "a1", "b2"]
        );
    }

    #[test]
    fn reports_conflicts_in_the_real_file() {
        let sandbox = Sandbox::new();
        sandbox.write(FIXTURE, ".ssh/config");
        let conflicts = sandbox
            .store
            .conflicts(&ManagedSshConfig::with_hosts([myserver()]).unwrap());
        assert_eq!(conflicts.len(), 2);
        assert!(conflicts.iter().all(|c| c.alias == "myserver"));
    }

    #[test]
    fn fnmatch_handles_wildcards_and_brackets() {
        let matches = |pattern: &str, name: &str| {
            fnmatch(
                &pattern.chars().collect::<Vec<_>>(),
                &name.chars().collect::<Vec<_>>(),
            )
        };
        assert!(matches("*.conf", "a.conf"));
        assert!(matches("*.conf", ".hidden.conf"));
        assert!(!matches("*.conf", "notes.txt"));
        assert!(matches("?.conf", "b.conf"));
        assert!(!matches("?.conf", "ab.conf"));
        assert!(matches("[a-c]*", "beta"));
        assert!(!matches("[!a-c]*", "beta"));
        assert!(matches("a\\*", "a*"));
        assert!(matches("[", "["));
        assert!(matches("*a*b", "xxaxxb"));
    }

    #[test]
    fn stops_following_includes_after_sixteen_levels() {
        let sandbox = Sandbox::new();
        sandbox.write("Include l0\n", ".ssh/config");
        for level in 0..20 {
            sandbox.write(
                &format!("Host h{level}\nInclude l{}\n", level + 1),
                &format!(".ssh/l{level}"),
            );
        }
        let aliases = sandbox.store.host_aliases();
        assert_eq!(aliases.len(), 16);
        assert_eq!(aliases.last().map(String::as_str), Some("h15"));
    }
}
