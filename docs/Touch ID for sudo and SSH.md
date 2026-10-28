# Unlock sudo and SSH with Touch ID natively

**Yes. macOS Tahoe 26.x supports both natively, so you don't need a custom app for either one.** For sudo, Apple ships `pam_tid.so` and an update-safe include file, `/etc/pam.d/sudo_local`. To enable it, copy Apple's template and uncomment one line ([Six Colors](https://sixcolors.com/post/2023/08/in-macos-sonoma-touch-id-for-sudo-can-survive-updates/)). For outbound SSH, `sc_auth create-ctk-identity -k p-256-ne -t bio` creates a non-exportable P-256 key in the Secure Enclave that requires biometrics. Apple's `/usr/lib/ssh-keychain.dylib` then exposes that key to the bundled OpenSSH as a FIDO-style `SecurityKeyProvider`, and it shows up as a resident `sk-ecdsa-sha2-nistp256@openssh.com` key ([arianvp gist](https://gist.github.com/arianvp/5f59f1783e3eaf1a2d4cd8e952bb4acf); [ewpratten.com](https://ewpratten.com/blog/ssh-secure-enclave/)). Both features work in Terminal.app without third-party code. The catch is that Apple documents them almost nowhere. `pam_tid` has no public Apple documentation. The SSH path appears only in the `sc_auth(8)` and `ssh-keychain(8)` man pages, with no release note or WWDC session. The setup below therefore marks every claim that rests only on community sources. To get "most secure" rather than just "working", add four things to the defaults. For sudo, set `timestamp_timeout=0` so that each tap authorizes one command, and if you use tmux, load `pam_reattach` from a root-owned copy. For SSH, use per-host `IdentitiesOnly` with no agent forwarding, and register a second, independent recovery key on every server, because Secure Enclave keys can't be backed up. A custom Swift agent is still worth building only if you want the Touch ID prompt to name the requesting process and destination host, or to enforce per-host policy. Neither built-in path offers that.

> **Scope and confidence.** These notes are current as of October 2026 for macOS Tahoe 26.x on Apple Silicon or T2 Macs. **[Undocumented]** marks behavior Apple does not publicly document. **[Unverified]** marks a claim the research could not confirm from a primary or Tahoe-dated source. Check both kinds on your own machine before relying on them.

## One line in sudo_local gives Touch ID sudo that survives updates

Since Sonoma 14, Apple's stock `/etc/pam.d/sudo` has started its auth stack with `auth include sudo_local`. Apple also ships `/etc/pam.d/sudo_local.template` with a commented-out `pam_tid.so` line and the header comment "local config file which survives system update" ([Siddhant-K-code gist](https://gist.github.com/Siddhant-K-code/6b653f4f356c06da9dd80ed33182cca2); [1Password Community](https://www.1password.community/developers-69/psa-macos-sonoma-and-sudo-biometrics-12789)). Before Sonoma, people edited `/etc/pam.d/sudo` directly, and even minor updates overwrote the change ([Apple Discussions](https://discussions.apple.com/thread/255182555)). The stock file on Sonoma 14.5 reads:

```
# sudo: auth account password session
auth       include        sudo_local
auth       sufficient     pam_smartcard.so
auth       required       pam_opendirectory.so
account    required       pam_permit.so
password   required       pam_deny.so
session    required       pam_permit.so
```

The ordering preserves the password fallback. `sudo_local` is evaluated first. A successful `sufficient` `pam_tid` ends the auth stack. A cancel or failure falls through to the smartcard module and then to `pam_opendirectory` (the password prompt). The same template appears on a clean macOS 15 install ([automatica](https://automatica-astro.pages.dev/2025/08/enable-touch-id-for-authentication-in-sudo)). **No source reports any Tahoe-specific change, and Tahoe has no System Settings toggle for this.** Confirm the include line on your machine before relying on it. **[Unverified on 26.x from a primary source]**

The setup works as-is in Terminal.app. iTerm2 has historically needed Settings → Advanced → "Allow sessions to survive logging out and back in" set to No ([DEV/equiman](https://dev.to/equiman/how-to-use-macos-s-touch-id-on-terminal-5fhg)). Whether current iTerm2 releases still need this is **[Unverified]**. Inside tmux or GNU screen, `pam_tid` fails because those servers outlive the GUI login session that Touch ID is bound to. `pam_reattach`, loaded as `optional` before `pam_tid`, moves the process back into the per-session bootstrap namespace ([pam_reattach README](https://www.github.com/fabianishere/pam_reattach)). Ghostty and VS Code have only anecdotal reports. They should behave like Terminal.app because they are ordinary GUI-session apps, with the same tmux caveat.

### Security posture: a generic prompt that authorizes whatever asked

The Touch ID sheet that `pam_tid` shows does not display the command being authorized. No source shows the argv appearing. By contrast, third-party tools such as cc-sudo advertise per-command display as a differentiator ([yasyf/cc-sudo](https://github.com/yasyf/cc-sudo)). As a result, any process running as you can call `sudo <payload>` and wait for a reflexive tap. Touch ID trades phishing-by-typing for phishing-by-tapping. The sudo timestamp cache makes this worse: after one legitimate tap, malware can reuse the cached ticket without any prompt for the default 5 minutes ([sudoers_timestamp(5)](https://man7.org/linux/man-pages/man5/sudoers_timestamp.5.html)). The DISA STIG for macOS 26 Tahoe already requires `Defaults timestamp_timeout=0` so that every sudo re-authenticates ([Tenable: DISA STIG macOS 26 v1r2](https://www.tenable.com/audits/items/DISA_STIG_Apple_macOS_26_Tahoe_v1r2.audit:d279fc6463a0338d7817fd9c85d3b6d1)). With Touch ID, that requirement costs you almost nothing. Each tap authorizes exactly one command, and there is never a ticket to piggy-back on.

The second risk is specific to `pam_reattach`. The common guidance points `sudo_local` at `/opt/homebrew/lib/pam/pam_reattach.so` ([pam_reattach README](https://www.github.com/fabianishere/pam_reattach)). On Apple Silicon, `/opt/homebrew` is writable by your user. Any user-level process could therefore replace that `.so`, and its code would run inside setuid-root `sudo` the next time you elevate. That is a silent local privilege escalation. None of the popular guides mention it. The fix is to copy the module to a root-owned directory and reference it there. Also avoid the screen-sharing override `defaults write com.apple.security.authorization ignoreArd -bool TRUE` that circulates in gist comments ([farhaduneci gist](https://gist.github.com/farhaduneci/10e30162eceb12a87bcee92b247b8808)). It disables the safeguard that suppresses biometric prompts while a remote party may be watching or controlling the screen ([Time Doctor KB](https://support.timedoctor.com/knowledge/touch-id-not-working-when-screen-recording-is-active)).

### Hardened sudo setup, step by step

> **Lockout guard.** Before you touch any PAM file, open a second terminal window and run `sudo -s`, then leave that root shell open until verification passes. If a PAM edit breaks sudo, use that shell to fix it. Finder's Go to Folder (⇧⌘G → `/etc/pam.d`) can also delete `sudo_local` with an admin prompt, because Finder's authentication does not go through sudo ([automatica](https://automatica-astro.pages.dev/2025/08/enable-touch-id-for-authentication-in-sudo)).

**Step 1: Inspect the baseline.**

```sh
cat /etc/pam.d/sudo                     # must contain: auth  include  sudo_local
cat /etc/pam.d/sudo_local.template
ls -l /etc/pam.d/sudo_local 2>/dev/null # usually absent on a fresh system
sudo grep -rn timestamp_timeout /etc/sudoers /etc/sudoers.d/ 2>/dev/null
```

**Step 2 (tmux/screen users only): Install pam_reattach as a root-owned copy.**

```sh
brew install pam-reattach

# The target directory and every ancestor must be root-owned and not group/world-writable.
ls -ld /usr/local /usr/local/lib
sudo install -d -o root -g wheel -m 0755 /usr/local/lib/pam

# 'install' follows the Homebrew symlink and writes a root-owned, read-only copy.
sudo install -o root -g wheel -m 0444 \
  /opt/homebrew/lib/pam/pam_reattach.so /usr/local/lib/pam/pam_reattach.so
shasum -a 256 /usr/local/lib/pam/pam_reattach.so   # record this; re-check after any change

# Optional: remove the Homebrew copy so 'brew upgrade' can never be mistaken for the live module.
# brew uninstall pam-reattach
```

> **Callout: root-owned copy.** If `ls -ld` shows `/usr/local` or `/usr/local/lib` owned by your user, pick another root-owned path such as `/opt/pam`, created with `install -d -o root -g wheel -m 0755`. This happens on Macs migrated from Intel Homebrew. Upgrades become manual, so re-copy only after reviewing the new release. Homebrew-installed `pam_reattach` loading inside Apple-signed sudo is widely reported to work. Whether library validation would block a differently signed build is **[Unverified]**.

**Step 3: Write `/etc/pam.d/sudo_local`.** `ignore_ssh` turns `pam_reattach` into a no-op when `SSH_CLIENT`, `SSH_CONNECTION` or `SSH_TTY` is set. This covers the case of tmux running inside an SSH session, where `pam_tid` might otherwise not detect the remote context ([pam_reattach README](https://www.github.com/fabianishere/pam_reattach)).

```sh
# With pam_reattach (tmux users):
sudo tee /etc/pam.d/sudo_local >/dev/null <<'EOF'
# sudo_local: local config file which survives system update and is included for sudo
auth       optional       /usr/local/lib/pam/pam_reattach.so ignore_ssh
auth       sufficient     pam_tid.so
EOF

# Without pam_reattach (Apple-only code path, preferred if you don't need tmux):
# sed -e 's/^#auth/auth/' /etc/pam.d/sudo_local.template | sudo tee /etc/pam.d/sudo_local >/dev/null

sudo chown root:wheel /etc/pam.d/sudo_local
sudo chmod 0444 /etc/pam.d/sudo_local
```

| Line | Control flag | Why this flag |
|---|---|---|
| `pam_reattach.so ignore_ssh` | `optional` | A module bug or missing file can't lock you out; the README explicitly warns against `required` ([pam_reattach](https://www.github.com/fabianishere/pam_reattach)) |
| `pam_tid.so` | `sufficient` | Success ends the auth stack. Failure or cancel falls through to the password, so a broken sensor or closed lid never blocks sudo |
| *(never)* `pam_tid.so` | `required` | Would make Touch ID mandatory and remove the password fallback, which is a lockout risk ([erikw](https://erikw.netlify.app/blog/tech/finger-of-god-using-sudo-with-touchid-on-macos)) |

**Step 4: Add the sudoers drop-in.** Always edit through `visudo`, which refuses to save a file that fails to parse. The STIG fix uses `/etc/sudoers.d/mscp`, which confirms that macOS reads the `sudoers.d` include directory. Its fix also deletes any other `timestamp_timeout` lines first, because sudoers applies the last matching `Defaults` ([Tenable STIG](https://www.tenable.com/audits/items/DISA_STIG_Apple_macOS_26_Tahoe_v1r2.audit:d279fc6463a0338d7817fd9c85d3b6d1)).

```sh
sudo visudo -f /etc/sudoers.d/10-touchid-hardening
```

```
# /etc/sudoers.d/10-touchid-hardening   (root:wheel 0440)
# Every sudo invocation requires a fresh Touch ID tap or password; no reusable ticket.
Defaults timestamp_timeout=0
```

```sh
sudo chmod 0440 /etc/sudoers.d/10-touchid-hardening
sudo visudo -c        # every file must report "parsed OK"
```

If you accept a short cache instead of `0`, add `Defaults timestamp_type=tty` so a ticket can't cross terminals ([sudoers_timestamp(5)](https://leancrew.com/all-this/man/man5/sudoers_timestamp.html)). The macOS default for `timestamp_type` is **[Unverified]**.

### Verification and rollback

| Check | Command | Expected result |
|---|---|---|
| Touch ID path | `sudo -k; sudo true` | Touch ID sheet instead of `Password:` ([andrewbaker.ninja](https://andrewbaker.ninja/2023/10/26/enable-touch-id-for-sudo-on-macos-that-survives-updates/)) |
| No ticket reuse | `sudo true` twice in a row | Prompts both times |
| Password fallback | `sudo true`, then Cancel on the sheet | Falls back to `Password:` |
| tmux | Repeat the first check inside `tmux` | Touch ID sheet (only with `pam_reattach`) |
| SSH into this Mac | `ssh localhost`, then `sudo true` | Password prompt, never a sheet on the console |
| File hygiene | `ls -l /etc/pam.d/sudo_local /usr/local/lib/pam/pam_reattach.so` | `root wheel`, `-r--r--r--` |
| Policy | `sudo -l` | Lists `timestamp_timeout=0` |

To roll back, run `sudo rm /etc/pam.d/sudo_local` (this restores password-only sudo, per [mac9sb](https://www.mintlify.com/mac9sb/config/setup/touchid)), `sudo rm /etc/sudoers.d/10-touchid-hardening`, and `sudo rm /usr/local/lib/pam/pam_reattach.so`, then run `sudo visudo -c` again. Over SSH *into* the Mac, Touch ID is unavailable by design ("you can't send your fingerprint through the internet", per [mattrajca/sudo-touchid #19](https://www.github.com/mattrajca/sudo-touchid/issues/19)), and sudo falls back to the password. How `pam_tid` detects SSH is **[Undocumented]**. Treat Apple Watch modules such as `pam-watchid` as a separate trust decision. They are third-party code inside setuid sudo, the most active fork installs with curl-pipe-to-bash, and on macOS 15+ it uses the `...BiometricsOrCompanion` policy, which widens the set of devices that can grant root ([mostpinkest/pam-watchid mirror](https://github.liu-huaimin.workers.dev/mostpinkest/pam-watchid/blob/main/README.md)). Clamshell behavior with a Magic Keyboard with Touch ID has no reliable source. Also note that anyone who can enroll a new fingerprint can then pass `sudo`. Enrolling one requires your password in System Settings.

## Three commands give the Secure Enclave an SSH key that never leaves the chip

Apple's `ssh-keychain.dylib` used to be a PKCS#11 smartcard shim. It now also implements OpenSSH's `SecurityKeyProvider` interface. By default it offers ECDSA P-256 identities from CryptoTokenKit as resident security keys ([ssh-keychain(8)](https://keith.github.io/xcode-man-pages/ssh-keychain.8.html)). `sc_auth(8)` exposes `create-ctk-identity` with `-k p-256-ne` (the `-ne` suffix means "non-exportable") and `-t bio` (biometric protection) ([sc_auth(8)](https://keith.github.io/xcode-man-pages/sc_auth.8.html)). The private key is generated inside the Secure Enclave and never leaves it. Malware running as you can at most request signatures, each one gated by Touch ID. It can never exfiltrate the key. Each use reportedly prompts for Touch ID, including every `git push` ([zeli.app summary of nmattia.com](https://zeli.app/story/50175882)) and every git commit signature ([HN](https://hn.jakechampion.name/item/46025721)).

> **Callout: version disputes.** Evan Pratten says the feature "depends on an update to CryptoTokenKit in macOS 26" ([ewpratten.com](https://ewpratten.com/blog/ssh-secure-enclave/)). Hacker News commenters report it working on Sequoia ([HN](https://hn.jakechampion.name/item/46025721)), and the mechanism was first described on openssh-unix-dev in July 2024 ([openssh-unix-dev](https://lists.mindrot.org/pipermail/openssh-unix-dev/2024-July/041451.html)). On Tahoe 26.x it works either way. Bundled OpenSSH is reported as `OpenSSH_10.0p1` on 26.3, according to one third-party source ([macos-tahoe.com](https://macos-tahoe.com/blog/macos-tahoe-26-3-ssh-changes-case-sensitivity-fix-guide-2026/)). **[Unverified]**: whether the signature is required per use or briefly cached by LocalAuthentication; whether `-t bio` binds to the *current* fingerprint set or any enrolled finger; whether Apple Watch can substitute for Touch ID.

### Constraints you'll hit

| Constraint | Detail | Source |
|---|---|---|
| Key type | Only `p-256-ne` (or exportable `p-256`). `p-384-ne` gives no prompt or key, because OpenSSH sk types are P-256 and Ed25519 only, and there's no SE Ed25519 | [arianvp gist](https://gist.github.com/arianvp/5f59f1783e3eaf1a2d4cd8e952bb4acf); [HN](https://hn.jakechampion.name/item/46025721) |
| Server | Must accept `sk-ecdsa-sha2-nistp256@openssh.com`, which needs OpenSSH ≥ 8.2. Some network gear and enterprise algorithm allowlists reject it | [Ubuntu docs](https://ubuntu.com/server/docs/how-to/security/two-factor-authentication-with-u2f-or-fido/) |
| Key generation | Generate with `sc_auth`. The dylib does not implement enroll, so `ssh-keygen -t ecdsa-sk -w …` is unsupported | [openssh-unix-dev](https://lists.mindrot.org/pipermail/openssh-unix-dev/2024-July/041451.html) |
| PIN prompt | OpenSSH asks for a meaningless PIN. Empty is accepted, or suppress it with `SSH_ASKPASS_REQUIRE=force SSH_ASKPASS=true` | [ewpratten.com](https://ewpratten.com/blog/ssh-secure-enclave/) |
| Multiple keys | `ssh-keygen -K` wrote every key to `id_ecdsa_sk_rk` (FB21992630, reported fixed in a later beta). Rename after each download | [arianvp gist](https://gist.github.com/arianvp/5f59f1783e3eaf1a2d4cd8e952bb4acf) |
| Attestation | None. A server can't prove the key is hardware-bound (one key was even created in a UTM VM) | [arianvp gist](https://gist.github.com/arianvp/5f59f1783e3eaf1a2d4cd8e952bb4acf) |
| Expiry | The "Valid To" date that `sc_auth` reports is not enforced by OpenSSH | [ewpratten.com](https://ewpratten.com/blog/ssh-secure-enclave/) |
| Backup | Non-exportable means a new Mac needs new keys. Register a recovery key | [Secretive FAQ](https://github.forks.workers.dev/maxgoedjen/secretive/blob/main/FAQ.md) |
| Homebrew OpenSSH | Ignored `SSH_SK_PROVIDER` as of 2024 but honored `-oSecurityKeyProvider=`. Prefer `/usr/bin/ssh` | [openssh-unix-dev](https://lists.mindrot.org/pipermail/openssh-unix-dev/2024-July/041451.html) |

### Hardened native SSH setup, step by step

**Step 1: Create the key.** The command shows a Touch ID prompt. Use one key per trust domain, for example personal and work.

```sh
sc_auth create-ctk-identity -l ssh-personal -k p-256-ne -t bio
sc_auth list-ctk-identities            # note the public key hash
sc_auth list-ctk-identities -t ssh     # SSH fingerprint view
```

**Step 2: Download the key stub and public key into `~/.ssh`.** The stub holds a key handle, not secret material. `ssh-keygen` writes to the current directory.

```sh
cd ~/.ssh
SSH_ASKPASS_REQUIRE=force SSH_ASKPASS=true \
  ssh-keygen -w /usr/lib/ssh-keychain.dylib -K
mv id_ecdsa_sk_rk     id_se_personal
mv id_ecdsa_sk_rk.pub id_se_personal.pub
chmod 600 id_se_personal; chmod 644 id_se_personal.pub
```

**Step 3: Make the provider the default for ssh tooling.** Put this in `~/.zprofile` ([arianvp gist](https://gist.github.com/arianvp/5f59f1783e3eaf1a2d4cd8e952bb4acf)):

```sh
export SSH_SK_PROVIDER=/usr/lib/ssh-keychain.dylib
```

**Step 4: Write `~/.ssh/config` with explicit key binding, no forwarding, and no agent fallback.** Two settings do the main work. `IdentitiesOnly yes` stops ssh from offering every key it knows, which also avoids tripping `MaxAuthTries` ([1Password SSH agent docs](https://www.1password.dev/ssh/agent)). `ForwardAgent no` prevents a compromised server from borrowing your agent. Agent forwarding "inherently creates a transitive trust relationship" ([RFC 9987 draft, Security Considerations](https://www.ietf.org/archive/id/draft-ietf-sshm-ssh-agent-07.html)). The SE key is used straight from the stub through the provider, never through an agent, so `IdentityAgent none` removes the agent path entirely for these hosts.

```sshconfig
# ~/.ssh/config  -- first match wins, so specific hosts go above Host *

Host github.com
    User git
    IdentityFile ~/.ssh/id_se_personal
    SecurityKeyProvider /usr/lib/ssh-keychain.dylib
    IdentitiesOnly yes
    IdentityAgent none

Host prod-*.example.com
    IdentityFile ~/.ssh/id_se_work
    SecurityKeyProvider /usr/lib/ssh-keychain.dylib
    IdentitiesOnly yes
    IdentityAgent none
    # Optional, convenience vs exposure: one Touch ID per 10-minute multiplexed session.
    # ControlMaster auto
    # ControlPath ~/.ssh/cm-%C
    # ControlPersist 10m

Host *
    ForwardAgent no
    IdentitiesOnly yes
    SecurityKeyProvider /usr/lib/ssh-keychain.dylib
    StrictHostKeyChecking ask
    HashKnownHosts yes
```

> **[Unverified on 26.x]** No source tested this exact combination of `IdentityAgent none` with `SecurityKeyProvider` and a file stub. Each option is standard `ssh_config`, and file-stub use with `-o SecurityKeyProvider=` is documented ([ewpratten.com](https://ewpratten.com/blog/ssh-secure-enclave/)). If you keep `ControlPersist`, anything that can reach the control socket can open new sessions over the multiplexed connection without a new Touch ID tap, which defeats per-use approval for that window. HN suggests ControlPersist only to reduce prompt fatigue ([HN](https://hn.jakechampion.name/item/46025721)).

**Step 5: Install the public key on servers with restrictions.** Run `ssh-copy-id -i ~/.ssh/id_se_personal user@host`, or prepend options in `authorized_keys`, for example `restrict,from="203.0.113.0/24" sk-ecdsa-sha2-nistp256@openssh.com AAAA… se-personal`. Default `sshd` policy already requires the sk "user present" flag. Whether Apple's provider also sets the user-verification flag, which is needed for `verify-required` / `PubkeyAuthOptions verify-required` ([OpenSSH 8.4 release notes](https://www.openssh.org/txt/release-8.4)), is **[Unverified]**. Test it on one host before you enforce it.

**Step 6: Register a recovery key on every server.** Because the SE key dies with the Mac's logic board, a second independent credential is mandatory. The best choice is a YubiKey resident key, which is portable and attestable and needs a physical touch plus an optional PIN ([Filippo Valsorda](https://words.filippo.io/openssh-fido2/); [Ubuntu docs](https://ubuntu.com/server/docs/how-to/security/two-factor-authentication-with-u2f-or-fido/)):

```sh
# Uses Homebrew OpenSSH (libfido2). Apple's /usr/bin/ssh lacks built-in USB FIDO middleware (inference, unverified on 26.x).
/opt/homebrew/bin/ssh-keygen -t ecdsa-sk -O resident -O verify-required \
  -O application=ssh:recovery -f ~/.ssh/id_recovery_sk -C "recovery-yubikey"
# Append id_recovery_sk.pub to authorized_keys on every host; store the token offline.
```

A second Mac's SE key is an acceptable alternative. The exportable `-k p-256` variant (`sc_auth export-ctk-identity` → password-protected PKCS#12) is Apple's only native backup path, but it gives up the non-exportability that makes this design strong. One commenter also reported that an imported copy came out with a different public key ([arianvp gist](https://gist.github.com/arianvp/5f59f1783e3eaf1a2d4cd8e952bb4acf)).

**Verification and rollback.** Run `ssh-keygen -lf ~/.ssh/id_se_personal.pub` and check that it matches `sc_auth list-ctk-identities -t ssh`. Then run `ssh -v github.com`. You should see one Touch ID sheet, "Server accepts key" for the sk key, and successful authentication. Test the recovery key separately with `ssh -o IdentitiesOnly=yes -i ~/.ssh/id_recovery_sk host` before you depend on it. To roll back, remove the key line from each server's `authorized_keys` first, then run `sc_auth delete-ctk-identity -h <hash>` and delete the stub files. Deleting first would leave orphaned trust entries on the servers. For git signing, set `git config gpg.format ssh` and `user.signingKey ~/.ssh/id_se_personal.pub` with `SSH_SK_PROVIDER` exported. Every commit then costs a tap, which is painful during rebases ([HN](https://hn.jakechampion.name/item/46025721)).

## Native Secure Enclave keys beat vault agents on exfiltration and lose on portability

The options differ along three axes: where the key lives, whether approval happens per signature or per session, and what the server must support. The native path and Secretive both use non-exportable Secure Enclave keys. The native path uses the `sk-` key type and no third-party code. Secretive uses plain `ecdsa-sha2-nistp256` keys, which every server accepts. Secretive 3.x adds a Tahoe redesign, out-of-process XPC payload parsing, and post-quantum ML-DSA keys, but users report agent hangs on Tahoe ([newreleases.io v3.0.4](https://newreleases.io/project/github/maxgoedjen/secretive/release/v3.0.4); [HN](https://hn.jakechampion.name/item/46025721)). Password-manager agents hold exportable software keys in a synced vault. 1Password caches approval "per key, per application" by default until lock, quit or timeout ([1Password agent security](https://www.1password.dev/ssh/agent/security)). Bitwarden defaults to prompting "Always" ([Bitwarden](https://bitwarden.com/help/ssh-agent.md)).

| Approach | Key location | Exportable | Per-use approval | Shows requester / host | Server needs | Backup | Third-party code |
|---|---|---|---|---|---|---|---|
| **Native `sc_auth` p-256-ne + ssh-keychain.dylib** | Secure Enclave | No | Touch ID, reported per use **[caching unverified]** | No | OpenSSH ≥ 8.2 (`sk-ecdsa`) | None; enroll recovery key | None |
| Native `sc_auth` p-256 (exportable) | SE-wrapped, exportable .p12 | Yes | Touch ID | No | OpenSSH ≥ 8.2 | .p12 export | None |
| Secretive 3.x ([site](https://secretive.pages.dev)) | Secure Enclave | No | Touch ID / Watch, or notify-only (configurable) | Notification + prompt | Any (plain ecdsa-p256) | None | Yes, open source, attested builds |
| sod ([botanica](https://botanica.software/blog/ssh-logins-with-sod)) | Secure Enclave | No | Touch ID every signature (passcode fallback) | Not documented | Any | None | Yes, MIT |
| 1Password agent | Vault (software) | Yes | Cached per key per app by default | Yes, process and key | Any (Ed25519/RSA) | Synced | Yes, proprietary |
| Bitwarden agent | Vault (software) | Yes | "Always" by default | Not documented | Any (Ed25519) | Synced | Yes, open source |
| YubiKey `ecdsa-sk`/`ed25519-sk` | External token | No | Physical touch, + PIN with `verify-required` | No | OpenSSH ≥ 8.2 | Second token | libfido2 (Homebrew OpenSSH) |
| Short-lived certs (step-ca, Cloudflare Access, Tailscale SSH) | CA-issued, minutes to hours | n/a | At IdP login (passkey/Touch ID) | IdP-dependent | `TrustedUserCAKeys` or daemon | n/a | Yes |

Short-lived certificates address a different problem: they make *authorization* expire even though the key persists. `step ssh login --not-after 1h` signs a certificate and loads it into the agent ([smallstep](https://smallstep.com/docs/step-cli/reference/ssh/login/)). Cloudflare Access has issued certificates valid for two minutes ([Cloudflare blog](https://blog.cloudflare.com:443/public-keys-are-not-enough-for-ssh-security)). Tailscale SSH's check mode re-authenticates against the IdP on a `checkPeriod` that can be set as low as one minute or to "always" ([Tailscale docs](https://tailscale.com/kb/1193/tailscale-ssh)). By default, step-cli generates its own software key, so pairing a certificate with an SE-held key needs a CA flow that signs an existing public key. Whether step-cli can sign a CTK public key directly is **[Unverified]**. For a fleet, the strongest posture layers these: a native SE key, a CA certificate with a TTL of hours, and IdP login gated by a platform passkey.

One weakness is shared by every FIDO-style and native option: **the Touch ID prompt doesn't name the destination.** OpenSSH's own agent-restriction write-up notes that FIDO touch prompts can be phished because they don't show where the signature goes ([OpenSSH agent-restrict](https://www.openssh.org/agent-restrict.html)). macOS also has no secure-desktop equivalent, so a fake dialog looks like a real one ([HN](https://hn.jakechampion.name/item/46025721)). Non-exportable keys keep this weakness bounded, because the attacker needs a fresh tricked tap for every signature. Closing it fully is the one remaining reason to write code.

## A custom Swift agent earns its keep only for prompt context and per-host policy

Build your own agent only if you need behavior the native path lacks. One example is a Touch ID reason string such as "Sign in to prod-db-1 (SHA256:…) with *work* key, requested by `ssh` under iTerm2 (Team ID …)". Another is per-host rules: allow `github.com` with a short reuse window, always require a fresh tap for `prod-*`, refuse forwarded requests outright. A third is plain `ecdsa-sha2-nistp256` keys for servers that reject `sk-`. Secretive already covers the third, so the first two justify a new build. A custom PAM module for sudo does not justify one. It would run inside setuid-root sudo, outside the GUI session, and duplicate Apple-signed `pam_tid`. At most, a companion app could write `sudo_local` as shown above after asking the user.

### Architecture

| Component | Responsibility | Key APIs / constraints |
|---|---|---|
| `TouchAgent.app` (SwiftUI) | Create, list and delete keys; edit policy; export `.pub` | CryptoKit, data-protection keychain |
| `TouchAgentHelper.app` (LSUIElement login item) | Owns the socket, enforces policy, shows prompts | `SMAppService.loginItem(identifier:)` ([Apple](https://developer.apple.com/documentation/servicemanagement/smappservice.md)). Must run in the user session, because launchd daemons can't use the data-protection keychain ([TN3137](https://developer.apple.com/documentation/technotes/tn3137-on-mac-keychains.md)) |
| `AgentParser.xpc` | Parses untrusted wire bytes in isolation | Mirrors Secretive 3's out-of-process parsing ([v3.0.0 notes](https://newreleases.io/project/github/maxgoedjen/secretive/release/v3.0.0)) |
| `AgentKit` (SwiftPM) | `AgentWire` codec (pure, Linux-testable), `AgentKeys`, `AgentServer` | Swift 6 strict concurrency; signing behind an `actor` that serializes prompts |

**Keys.** Create keys with `SecureEnclave.P256.Signing.PrivateKey(compactRepresentable:accessControl:authenticationContext:)` ([Apple](https://developer.apple.com/tutorials/data/documentation/cryptokit/secureenclave/p256/signing/privatekey.md)). Persist only the key's `dataRepresentation`, an encrypted blob that only the same Secure Enclave can use, as a `kSecClassGenericPassword` with `kSecUseDataProtectionKeychain: true` ([Apple: Storing CryptoKit Keys](https://developer.apple.com/documentation/cryptokit/storing-cryptokit-keys-in-the-keychain.md)). Choose `[.privateKeyUsage, .biometryCurrentSet]` for a "strict" mode: re-enrolling a finger kills the key, and there is no password fallback. `[.privateKeyUsage, .userPresence]` is the default mode, with password fallback that survives enrollment changes ([SecAccessControlCreateFlags](https://developer.apple.com/documentation/security/secaccesscontrolcreateflags.md)). Do not rely on `touchIDAuthenticationAllowableReuseDuration` for a "remember for N seconds" feature. It applies to device-unlock reuse, "not keychain retrieval authentications" ([Apple](https://developer.apple.com/documentation/localauthentication/lacontext/touchidauthenticationallowablereuseduration.md)). Whether a pre-evaluated `LAContext` satisfies later SE signing calls is **[Unverified]**, and a developer report found per-call prompts even with reuse set ([Dev Forums 812733](https://developer.apple.com/forums/thread/812733)).

**Wire protocol.** The agent protocol is now RFC 9987 (Standards Track, May 2026). Each message is framed as `uint32 length || byte type || contents`. A minimal agent handles REQUEST_IDENTITIES (11) → IDENTITIES_ANSWER (12) and SIGN_REQUEST (13) → SIGN_RESPONSE (14), and returns FAILURE (5) for every other message type. It answers unknown EXTENSION (27) requests with EXTENSION_FAILURE (28) ([RFC 9987](https://www.rfc-editor.org/rfc/rfc9987.xml)). To get the destination host into the prompt, parse OpenSSH's non-RFC `session-bind@openssh.com` extension. It carries the server host key, the session ID, the server's signature over that ID, and an `is_forwarding` flag ([PROTOCOL.agent](https://fuchsia.googlesource.com/third_party/openssh-portable/+/refs/heads/main/PROTOCOL.agent)). **Use the host key for policy only after verifying that signature. Otherwise use it for display only.** Keys and signatures follow RFC 5656: the key blob is `string "ecdsa-sha2-nistp256" || string "nistp256" || string Q`, and the signature blob is `mpint r || mpint s` ([RFC 5656](https://gopherproxy.meulie.net/gopher.rbfh.de/0/RFC/rfc5656.txt)). CryptoKit's `rawRepresentation` is `r || s` ([swift-crypto](https://apple.github.io/swift-crypto/docs/current/Crypto/Enums/P256/Signing/ECDSASignature.html)), so split it at byte 32 and mpint-encode each half.

**Socket.** Use BSD sockets, not `NWListener`. Network.framework inherits the 104-byte `sun_path` limit, which long sandbox container paths exceed ([Dev Forums 756756](https://developer.apple.com/forums/thread/756756)). After `accept()`, read `LOCAL_PEERTOKEN` (an audit token, `0x006` at `SOL_LOCAL`) rather than a PID, because "protecting against PID reuse is one of the primary reasons that audit tokens exist." Then pass the token to `SecCodeCopyGuestWithAttributes` to get the caller's signing identity ([Dev Forums 850777](https://developer.apple.com/forums/thread/850777); [xnu un.h](https://git.saurik.com/apple/xnu.git/blame/HEAD:/bsd/sys/un.h)).

### Key code skeleton

```swift
import CryptoKit, LocalAuthentication, Security, Foundation

// MARK: Key creation (UI app). Store key.dataRepresentation in the data-protection keychain.
func makeKey(strict: Bool) throws -> SecureEnclave.P256.Signing.PrivateKey {
    var err: Unmanaged<CFError>?
    let flags: SecAccessControlCreateFlags = strict ? [.privateKeyUsage, .biometryCurrentSet]
                                                    : [.privateKeyUsage, .userPresence]
    guard let ac = SecAccessControlCreateWithFlags(kCFAllocatorDefault,
            kSecAttrAccessibleWhenUnlockedThisDeviceOnly, flags, &err)
    else { throw err!.takeRetainedValue() as Error }
    return try SecureEnclave.P256.Signing.PrivateKey(compactRepresentable: false,
                                                     accessControl: ac, authenticationContext: nil)
}

// MARK: Wire encoding (RFC 4251 string/mpint, RFC 5656 ecdsa)
extension Data {
    mutating func sshString(_ d: Data) { var n = UInt32(d.count).bigEndian
        append(Data(bytes: &n, count: 4)); append(d) }
    mutating func sshString(_ s: String) { sshString(Data(s.utf8)) }
}
func mpint(_ bytes: some Collection<UInt8>) -> Data {      // two's-complement, minimal length
    var b = Array(bytes.drop { $0 == 0 })
    if let f = b.first, f & 0x80 != 0 { b.insert(0, at: 0) } // keep positive
    var out = Data(); out.sshString(Data(b)); return out    // zero -> length 0
}
func publicKeyBlob(_ pk: P256.Signing.PublicKey) -> Data {
    var d = Data(); d.sshString("ecdsa-sha2-nistp256"); d.sshString("nistp256")
    d.sshString(pk.x963Representation)                      // 65 bytes: 0x04||X||Y
    return d
}
func signResponse(_ sig: P256.Signing.ECDSASignature) -> Data {
    let raw = sig.rawRepresentation                         // r(32) || s(32)
    var inner = Data(); inner.append(mpint(raw.prefix(32))); inner.append(mpint(raw.suffix(32)))
    var sigBlob = Data(); sigBlob.sshString("ecdsa-sha2-nistp256"); sigBlob.sshString(inner)
    var body = Data([14]); body.sshString(sigBlob)          // SSH_AGENT_SIGN_RESPONSE
    var framed = Data(); framed.sshString(body); return framed
}

// MARK: Signing with a context-rich prompt (helper). Do NOT pre-hash; CryptoKit applies SHA-256.
actor Signer {
    func sign(blob: Data, payload: Data, host: String, keyName: String, requester: String)
        throws -> P256.Signing.ECDSASignature {
        let ctx = LAContext()                               // non-Sendable: create inside the actor
        ctx.localizedReason = "sign in to \(host) with “\(keyName)” (requested by \(requester))"
        let key = try SecureEnclave.P256.Signing.PrivateKey(dataRepresentation: blob,
                                                            authenticationContext: ctx)
        return try key.signature(for: payload)
    }
}

// MARK: Peer identity after accept(). Reject other UIDs; resolve the caller by audit token.
func peerIdentity(_ fd: Int32) -> (uid: uid_t, token: audit_token_t)? {
    var uid = uid_t(), gid = gid_t()
    guard getpeereid(fd, &uid, &gid) == 0, uid == getuid() else { return nil }
    var tok = audit_token_t(); var len = socklen_t(MemoryLayout<audit_token_t>.size)
    guard getsockopt(fd, SOL_LOCAL, LOCAL_PEERTOKEN, &tok, &len) == 0 else { return nil }
    return (uid, tok)   // -> SecCodeCopyGuestWithAttributes([kSecGuestAttributeAudit: tokData])
}
// Listener: umask(0o077); socket(AF_UNIX, SOCK_STREAM, 0); bind(path < 104 bytes); chmod 0600;
// listen(fd, 16); cap frames at 256 KiB; bounds-check every string; reject trailing bytes.
```

The entitlements and distribution follow from Apple's keychain rules. Data-protection keychain access groups are restricted entitlements that must be authorized by a provisioning profile embedded in an app-like bundle. Without them, calls fail with `-34018` ([TN3137](https://developer.apple.com/documentation/technotes/tn3137-on-mac-keychains.md); [Dev Forums 759481](https://developer.apple.com/forums/thread/759481)).

| Entitlement / setting | App | Helper | Note |
|---|---|---|---|
| `com.apple.security.app-sandbox` | true | true | Socket lives in the container; keep the path < 104 bytes with a short bundle ID |
| `keychain-access-groups` | `$(TeamIdentifierPrefix)com.example.touchagent` | same | Shared so the UI's keys are usable by the helper |
| Hardened runtime | on | on | No `get-task-allow` and no `disable-library-validation` in release builds. RFC 9987 asks agents to resist memory reading and core dumps |
| Developer ID profile + notarization | required | required | SwiftPM alone can't embed the profile; wrap the package in an Xcode app target |

Point clients at the agent with `IdentityAgent <socket>` per `Host` in `~/.ssh/config`. Don't overwrite launchd's `SSH_AUTH_SOCK` ([Secretive APP_CONFIG](https://github.fontlos.workers.dev/maxgoedjen/secretive/blob/main/APP_CONFIG.md)). Test the codec with Swift Testing. Include the mpint edge cases `[0x00,0x00,0x7f] → 00000001 7f` and `[0x80] → 00000002 0080`, and round-trip signature verification against a software `P256.Signing.PrivateKey` behind a `Signer` protocol so CI needs no Secure Enclave. Then integration-test with `SSH_AUTH_SOCK=<sock> ssh-add -L` and `ssh -vvv -o IdentityAgent=<sock> localhost`. Writing a custom OpenSSH SK-provider dylib instead (`sk_api_version`, `sk_enroll`, `sk_sign`, `sk_load_resident_keys` per [sk-api.h](https://fossies.org/linux/openssh/sk-api.h)) is a poor trade. It runs inside `ssh` with no UI process of its own, it inherits the host's entitlements, and Apple's `ssh-keychain.dylib` already fills that role.

## Conclusion

The question "do I need to build something?" has flipped since 2023. Apple now ships both halves: a persistent PAM include for sudo, and a Secure Enclave-to-OpenSSH bridge for SSH. The remaining work is hardening the defaults, not writing code. The two most important changes are ones popular guides omit. Set `timestamp_timeout=0`, which turns Touch ID from a convenience that widens the attack surface into one-tap-per-command consent. Load `pam_reattach` only from a root-owned path, which closes a user-to-root escalation that the standard `/opt/homebrew` instructions create. On the SSH side, the non-exportable SE key moves the threat from key theft to tricking you into tapping. That raises the attacker's cost for every signature, but it does not remove the risk, because neither native prompt says what you are approving.

That gap is the real frontier. Apple's documentation does not cover either feature, Apple provides no attestation for SE keys, and neither native prompt can show the command or the host. Expect the next meaningful improvements to come from Apple documenting and extending `ssh-keychain.dylib`, from OpenSSH-side destination binding reaching the prompt, or from a small custom agent that uses `session-bind` and audit tokens to state exactly who is asking and where the signature goes. Until then, the practical recipe is the native path with a hardware recovery key on every server. Build the Swift agent only if per-host policy and prompt context matter more to you than having zero third-party code.
