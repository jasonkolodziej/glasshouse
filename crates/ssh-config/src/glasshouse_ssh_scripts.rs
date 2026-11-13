//! Scripts Glasshouse runs in a visible tab, so you see every command before it runs.

use std::collections::HashSet;

use crate::{ManagedSshHost, SshConfigError, SshPublicKey, glasshouse_ssh};

const CREATE_SECURE_ENCLAVE_KEY: &str = r#"set -euo pipefail
if [ -e {{key}} ] || [ -e {{key}}.pub ]; then
  echo "{{key}} already exists. Choose another key name in Settings › SSH Keys."; exit 1
fi
# 1. Create a P-256 key inside the Secure Enclave: non-exportable (-ne), Touch ID required (bio).
sc_auth create-ctk-identity -l {{name}} -k p-256-ne -t bio
# 2. Download the key *handle* (no secret material) into a scratch folder, then move it into place.
tmp="$(mktemp -d)"; cd "$tmp"
ssh-keygen -w {{provider}} -K -N ""     # press Return if asked for a PIN
install -d -m 0700 ~/.ssh
mv id_ecdsa_sk_rk {{key}} && mv id_ecdsa_sk_rk.pub {{key}}.pub
chmod 0600 {{key}}; cd - >/dev/null; rmdir "$tmp" 2>/dev/null || true
echo; echo "Public key:"
cat {{key}}.pub
echo; echo "Next: Settings › SSH Keys › Add Host…, then Install Keys… to put it on a server."
echo "Add a backup key for each host too: Secure Enclave keys cannot be backed up.""#;

const INSTALL_KEYS: &str = r#"set -euo pipefail
# Without the managed entry in effect, the test login below could succeed with another key.
if ! ssh -G {{alias}} | grep -qx 'pubkeyacceptedalgorithms {{algorithm}}'; then
  echo "Glasshouse's settings for {{host_alias}} are not in effect."
  echo "In Settings › SSH Keys, add the Include to ~/.ssh/config, then try again."; exit 1
fi
echo "Installing keys on {{host_alias}} ({{target}}):"
{{listing}}
echo "This step logs in the way you do today (password or another key), not with Touch ID."
report="$(ssh -o IdentityAgent=SSH_AUTH_SOCK -o IdentitiesOnly=no \
  -o PubkeyAcceptedAlgorithms=+{{algorithm}} {{alias}} 'sh -s' <<'GLASSHOUSE_REMOTE'
{{remote}}
GLASSHOUSE_REMOTE
)"
printf '%s\n' "$report" | awk -F'\t' '$1 == "added" { print "  added: " $2 } $1 == "present" { print "  already there: " $2 }'

echo; echo "Testing: this login must ask for Touch ID."
if ssh {{alias}} true; then echo "✓ {{host_alias}}: Touch ID login works"
else echo "✗ {{host_alias}}: the Touch ID login failed. Run: ssh -v {{host_alias}}"; exit 1; fi

notouch="$(printf '%s\n' "$report" | awk -F'\t' '$1 == "notouch"')"
if [ -z "$notouch" ]; then echo "✓ No other keys on {{host_alias}} log in without Touch ID or a security-key tap."; exit 0; fi
echo; echo "These other keys on {{host_alias}} log in without Touch ID or a security-key tap:"
printf '%s\n' "$notouch" | awk -F'\t' '{ print "  - " $3 $4 }'
"#;

const OFFER_REMOVAL: &str = r#"read -r -p "Remove them from {{host_alias}}? A copy of authorized_keys stays on the server. [y/N] " answer
case "$answer" in [yY]*) ;; *) echo "Kept."; exit 0 ;; esac
blobs="$(printf '%s\n' "$notouch" | awk -F'\t' '{ printf "%s ", $2 }')"
echo "Removing them over the Touch ID login."
ssh {{alias}} "sh -s $blobs" <<'GLASSHOUSE_REMOVE'
{{removal}}
GLASSHOUSE_REMOVE"#;

const NO_BACKUP: &str = r#"echo "Glasshouse won't offer to remove them: {{host_alias}} has no backup key, so without this Mac"
echo "they may be your only way in. Add a backup key in Settings › SSH Keys, then run Install Keys… again.""#;

const REMOTE_INSTALL: &str = r#"set -eu
umask 077
f="$HOME/.ssh/authorized_keys"
mkdir -p "$HOME/.ssh" && chmod 700 "$HOME/.ssh"
touch "$f" && chmod 600 "$f"
if [ -s "$f" ] && [ -n "$(tail -c 1 "$f")" ]; then echo >> "$f"; fi
add() {
  if grep -qF -- "$2" "$f"; then printf 'present\t%s\n' "$3"
  else printf '%s\n' "$1" >> "$f"; printf 'added\t%s\n' "$3"; fi
}
{{adds}}
awk -v keep='{{keep}}' '
  BEGIN { n = split(keep, k, " "); for (i = 1; i <= n; i++) kept[k[i]] = 1 }
  /^[ \t]*(#|$)/ { next }
  {
    for (i = 1; i < NF; i++) if ($i ~ /^(ssh-|ecdsa-sha2-|sk-)/) break
    if (i >= NF) next
    type = $i; blob = $(i + 1)
    if ((blob in kept) || blob !~ /^[A-Za-z0-9+\/=]+$/) next
    options = ""; for (j = 1; j < i; j++) options = options $j " "
    comment = ""; for (j = i + 2; j <= NF; j++) comment = comment " " $j
    gsub(/[^ -~]/, "?", comment); gsub(/[^ -~]/, "?", type)
    if (type !~ /^sk-/ || options ~ /no-touch-required/) printf "notouch\t%s\t%s\t%s\n", blob, type, comment
  }' "$f""#;

/// Runs on the server with `sh -s BLOB…`. Copies `authorized_keys` aside, then removes every line holding one of
/// the given key data strings.
pub(crate) const REMOTE_REMOVAL_SCRIPT: &str = r#"set -eu
f="$HOME/.ssh/authorized_keys"
copy="$f.glasshouse-$(date +%Y%m%d%H%M%S)"
cp -p "$f" "$copy"
tmp="$f.glasshouse-tmp"
cp -p "$f" "$tmp"
for blob in "$@"; do grep -vF -- "$blob" "$tmp" > "$tmp.next" || true; mv "$tmp.next" "$tmp"; done
chmod 600 "$tmp" && mv "$tmp" "$f"
echo "Removed $# key(s). The previous file is $copy""#;

/// Replaces each `{{name}}` in `template` in one pass, so substituted values are never scanned again.
fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let replacement = after.find("}}").and_then(|end| {
            let name = &after[..end];
            values
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value, end + 2))
        });
        match replacement {
            Some((value, consumed)) => {
                out.push_str(value);
                rest = &after[consumed..];
            }
            None => {
                out.push_str("{{");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Creates a Secure Enclave identity with `sc_auth` and downloads its key handle to `~/.ssh/<name>`.
/// `ssh-keygen -K` may ask for a PIN interactively, which is why this runs in a tab.
pub fn create_secure_enclave_key(name: &str) -> Result<String, SshConfigError> {
    if !ManagedSshHost::is_valid_key_name(name) {
        return Err(SshConfigError::InvalidKeyName(name.to_owned()));
    }
    let key = format!("~/.ssh/{name}");
    Ok(fill(
        CREATE_SECURE_ENCLAVE_KEY,
        &[
            ("key", &key),
            ("name", name),
            ("provider", glasshouse_ssh::SECURITY_KEY_PROVIDER),
        ],
    ))
}

/// One key to put in the server's `authorized_keys`, and what to call it in messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstallItem {
    pub(crate) key: SshPublicKey,
    pub(crate) label: String,
}

/// Every key a host keeps on its server: the Secure Enclave key, its security key, then its backup keys.
pub(crate) fn install_items(
    host: &ManagedSshHost,
    touch_id_key: &SshPublicKey,
    security_key: Option<&SshPublicKey>,
) -> Vec<InstallItem> {
    let mut items = vec![InstallItem {
        key: touch_id_key.clone(),
        label: format!("Touch ID key ~/.ssh/{}", host.key_name),
    }];
    if let (Some(security_key), Some(configured)) = (security_key, &host.security_key) {
        items.push(InstallItem {
            key: security_key.clone(),
            label: format!("security key ~/.ssh/{}", configured.key_name),
        });
    }
    items.extend(
        host.parsed_backup_keys()
            .into_iter()
            .map(|key| InstallItem {
                label: format!("backup key {}", key.label()),
                key,
            }),
    );
    let mut seen = HashSet::new();
    items.retain(|item| seen.insert(item.key.blob().to_owned()));
    items
}

/// Installs the host's keys on the server, then logs in with Touch ID. Then it lists the server's keys that log
/// in without a touch (Touch ID or a security-key tap) and, only if the host has a backup, offers to remove them.
///
/// The first login has to work the way you already log in, so it undoes the managed entry's restrictions for
/// that one command: options on the command line win over the config file. The removal runs over the Touch ID
/// login, after that login has just worked.
pub fn install_keys(
    host: &ManagedSshHost,
    touch_id_key: &SshPublicKey,
    security_key: Option<&SshPublicKey>,
) -> Result<String, SshConfigError> {
    let host = host.validated()?;
    if touch_id_key.key_type() != glasshouse_ssh::SECURE_ENCLAVE_ALGORITHM {
        return Err(SshConfigError::MissingPublicKey(host.key_name.clone()));
    }
    let items = install_items(&host, touch_id_key, security_key);
    let alias = shell_quote(&host.alias);
    let target = format!(
        "{}{}{}",
        host.user
            .as_ref()
            .map(|user| format!("{user}@"))
            .unwrap_or_default(),
        host.host_name,
        host.port.map(|port| format!(":{port}")).unwrap_or_default()
    );
    let has_backup = host.security_key.is_some() || !host.backup_keys.is_empty();
    let listing = items
        .iter()
        .map(|item| format!("echo {}", shell_quote(&format!("  - {}", item.label))))
        .collect::<Vec<_>>()
        .join("\n");
    let remote = remote_install_script(&items);
    let values = [
        ("alias", alias.as_str()),
        ("host_alias", host.alias.as_str()),
        ("algorithm", glasshouse_ssh::SECURE_ENCLAVE_ALGORITHM),
        ("target", &target),
        ("listing", &listing),
        ("remote", &remote),
        ("removal", REMOTE_REMOVAL_SCRIPT),
    ];
    let mut script = fill(INSTALL_KEYS, &values);
    script += &fill(if has_backup { OFFER_REMOVAL } else { NO_BACKUP }, &values);
    Ok(script)
}

/// Runs on the server with `sh -s`. Appends each key unless its key data is already there (so running it twice
/// adds nothing), then prints one tab-separated line per key that logs in without a touch:
/// `notouch`, key data, type, comment. Keys in `items` are never listed. Prints only data, never runs it.
pub(crate) fn remote_install_script(items: &[InstallItem]) -> String {
    let adds = items
        .iter()
        .map(|item| {
            format!(
                "add {} {} {}",
                shell_quote(&item.key.line()),
                shell_quote(item.key.blob()),
                shell_quote(&item.label)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let keep = items
        .iter()
        .map(|item| item.key.blob())
        .collect::<Vec<_>>()
        .join(" ");
    fill(REMOTE_INSTALL, &[("adds", &adds), ("keep", &keep)])
}

/// Single-quotes `value` for a POSIX shell.
pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write as _;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::Path;
    use std::process::{Command, Stdio};

    use super::*;
    use crate::SecurityKey;
    use crate::test_support::{key_line, myserver};

    fn touch_id_key() -> SshPublicKey {
        SshPublicKey::parse(&key_line(
            glasshouse_ssh::SECURE_ENCLAVE_ALGORITHM,
            "you@mac",
            1,
        ))
        .unwrap()
    }

    fn backup_line() -> String {
        key_line("ssh-ed25519", "laptop", 2)
    }

    #[test]
    fn fill_does_not_rescan_substituted_values() {
        assert_eq!(
            fill("{{a}} {{b}} {{c", &[("a", "{{b}}"), ("b", "x")]),
            "{{b}} x {{c"
        );
    }

    #[test]
    fn install_script_checks_the_entry_then_installs_with_the_usual_login() {
        let script = install_keys(&myserver(), &touch_id_key(), None).unwrap();
        assert!(script.contains(
            "ssh -G 'myserver' | grep -qx 'pubkeyacceptedalgorithms sk-ecdsa-sha2-nistp256@openssh.com'"
        ));
        assert!(script.contains("ssh -o IdentityAgent=SSH_AUTH_SOCK -o IdentitiesOnly=no"));
        assert!(script.contains(
            "-o PubkeyAcceptedAlgorithms=+sk-ecdsa-sha2-nistp256@openssh.com 'myserver' 'sh -s' <<'GLASSHOUSE_REMOTE'"
        ));
        assert!(script.contains("\nGLASSHOUSE_REMOTE\n"));
        assert!(script.contains("if ssh 'myserver' true;"));
        assert!(script.contains("(you@10.0.0.5:22)"));
        assert!(script.contains("echo '  - Touch ID key ~/.ssh/glasshouse'"));
    }

    #[test]
    fn offers_to_remove_keys_only_when_the_host_has_a_backup() {
        let without = install_keys(&myserver(), &touch_id_key(), None).unwrap();
        assert!(!without.contains("read -r -p"));
        assert!(without.contains("has no backup key"));

        let host = myserver().with_backup_keys(vec![backup_line()]);
        let with = install_keys(&host, &touch_id_key(), None).unwrap();
        assert!(with.contains("read -r -p \"Remove them from myserver?"));
        assert!(with.contains("ssh 'myserver' \"sh -s $blobs\" <<'GLASSHOUSE_REMOVE'"));
        assert!(with.ends_with("\nGLASSHOUSE_REMOVE"));
        assert!(with.contains("echo '  - backup key laptop (ssh-ed25519)'"));
    }

    #[test]
    fn installs_the_security_key_alongside_and_counts_it_as_a_backup() {
        let host = myserver().with_security_key(Some(SecurityKey::new("id_ed25519_sk")));
        let security_key =
            SshPublicKey::parse(&key_line("sk-ssh-ed25519@openssh.com", "yubikey", 3)).unwrap();
        let script = install_keys(&host, &touch_id_key(), Some(&security_key)).unwrap();
        assert!(script.contains("echo '  - security key ~/.ssh/id_ed25519_sk'"));
        assert!(script.contains("read -r -p"));
    }

    #[test]
    fn refuses_a_key_that_is_not_a_secure_enclave_key() {
        let plain = SshPublicKey::parse(&backup_line()).unwrap();
        assert_eq!(
            install_keys(&myserver(), &plain, None),
            Err(SshConfigError::MissingPublicKey("glasshouse".into()))
        );
        let bad_host = ManagedSshHost::new("-x", "h", None, None, "k");
        assert!(install_keys(&bad_host, &touch_id_key(), None).is_err());
    }

    #[test]
    fn create_key_script_never_overwrites_a_key() {
        let script = create_secure_enclave_key("work").unwrap();
        assert!(script.contains("if [ -e ~/.ssh/work ] || [ -e ~/.ssh/work.pub ]; then"));
        assert!(script.contains("sc_auth create-ctk-identity -l work -k p-256-ne -t bio"));
        assert!(
            script
                .contains("mv id_ecdsa_sk_rk ~/.ssh/work && mv id_ecdsa_sk_rk.pub ~/.ssh/work.pub")
        );
        assert!(
            script.contains(
                "ssh-keygen -w /usr/lib/ssh-keychain.dylib -K -N \"\"     # press Return"
            )
        );
        assert_eq!(
            create_secure_enclave_key("../id_rsa"),
            Err(SshConfigError::InvalidKeyName("../id_rsa".into()))
        );
    }

    #[test]
    fn quotes_for_the_shell() {
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }

    // Server-side scripts, run with /bin/sh against a throwaway home the way `ssh host 'sh -s'` runs them.

    struct Remote {
        se_key: SshPublicKey,
        backup: SshPublicKey,
        everyday: String,
        yubikey: String,
        no_touch: String,
    }

    impl Remote {
        fn new() -> Self {
            Self {
                se_key: touch_id_key(),
                backup: SshPublicKey::parse(&backup_line()).unwrap(),
                everyday: key_line("ssh-ed25519", "id_someID", 4),
                yubikey: key_line("sk-ssh-ed25519@openssh.com", "yubikey", 5),
                no_touch: key_line("sk-ssh-ed25519@openssh.com", "kiosk", 6),
            }
        }

        fn items(&self) -> Vec<InstallItem> {
            vec![
                InstallItem {
                    key: self.se_key.clone(),
                    label: "Touch ID key ~/.ssh/glasshouse".into(),
                },
                InstallItem {
                    key: self.backup.clone(),
                    label: "backup key laptop".into(),
                },
            ]
        }
    }

    fn make_home(authorized_keys: &str) -> tempfile::TempDir {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir_all(home.path().join(".ssh")).unwrap();
        fs::write(home.path().join(".ssh/authorized_keys"), authorized_keys).unwrap();
        home
    }

    fn authorized_keys(home: &Path) -> String {
        fs::read_to_string(home.join(".ssh/authorized_keys")).unwrap()
    }

    fn run_shell(script: &str, home: &Path, arguments: &[&str]) -> String {
        let mut child = Command::new("/bin/sh")
            .arg("-s")
            .args(arguments)
            .env_clear()
            .env("HOME", home)
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(script.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        let text = String::from_utf8_lossy(&output.stdout).into_owned()
            + &String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{text}");
        text
    }

    #[test]
    fn adds_missing_keys_once_and_lists_keys_that_skip_the_touch() {
        let remote = Remote::new();
        // The Secure Enclave key is already there under another comment; the file has no final newline.
        let existing = format!(
            "# keys\n{}\nno-touch-required {}\n{}\n{} {} old-comment",
            remote.everyday,
            remote.no_touch,
            remote.yubikey,
            remote.se_key.key_type(),
            remote.se_key.blob()
        );
        let home = make_home(&existing);

        let output = run_shell(&remote_install_script(&remote.items()), home.path(), &[]);
        let lines: Vec<&str> = output.lines().collect();
        assert!(lines.contains(&"present\tTouch ID key ~/.ssh/glasshouse"));
        assert!(lines.contains(&"added\tbackup key laptop"));
        let flagged: Vec<Vec<&str>> = lines
            .iter()
            .filter(|line| line.starts_with("notouch\t"))
            .map(|line| line.split('\t').collect())
            .collect();
        let blobs: Vec<&str> = flagged.iter().map(|fields| fields[1]).collect();
        let everyday = SshPublicKey::parse(&remote.everyday).unwrap();
        let no_touch = SshPublicKey::parse(&remote.no_touch).unwrap();
        assert_eq!(blobs, [everyday.blob(), no_touch.blob()]);
        assert_eq!(flagged[0][3], " id_someID");

        let expected = format!("{existing}\n{}\n", remote.backup.line());
        assert_eq!(authorized_keys(home.path()), expected);
        run_shell(&remote_install_script(&remote.items()), home.path(), &[]);
        assert_eq!(authorized_keys(home.path()), expected);
        let mode = fs::metadata(home.path().join(".ssh/authorized_keys"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o7777, 0o600);
    }

    #[test]
    fn creates_the_file_when_the_server_has_none() {
        let remote = Remote::new();
        let home = make_home("");
        fs::remove_dir_all(home.path().join(".ssh")).unwrap();
        let output = run_shell(&remote_install_script(&remote.items()), home.path(), &[]);
        assert!(!output.contains("notouch"));
        assert_eq!(
            authorized_keys(home.path()),
            format!("{}\n{}\n", remote.se_key.line(), remote.backup.line())
        );
    }

    #[test]
    fn removes_only_the_chosen_keys_and_keeps_a_copy() {
        let remote = Remote::new();
        let existing = format!(
            "{}\n{}\nno-touch-required {}\n{}\n",
            remote.everyday,
            remote.se_key.line(),
            remote.no_touch,
            remote.backup.line()
        );
        let home = make_home(&existing);

        let everyday = SshPublicKey::parse(&remote.everyday).unwrap();
        let no_touch = SshPublicKey::parse(&remote.no_touch).unwrap();
        let output = run_shell(
            REMOTE_REMOVAL_SCRIPT,
            home.path(),
            &[everyday.blob(), no_touch.blob()],
        );
        assert!(output.starts_with("Removed 2 key(s)."));
        assert_eq!(
            authorized_keys(home.path()),
            format!("{}\n{}\n", remote.se_key.line(), remote.backup.line())
        );
        let copies: Vec<String> = fs::read_dir(home.path().join(".ssh"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| name.starts_with("authorized_keys.glasshouse-"))
            .collect();
        assert_eq!(copies.len(), 1);
        assert_eq!(
            fs::read_to_string(home.path().join(".ssh").join(&copies[0])).unwrap(),
            existing
        );
    }
}
