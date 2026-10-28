# Glasshouse

A native macOS terminal (Swift 6, macOS Tahoe 26+) that keeps Terminal.app's strengths — VoiceOver, prompt marks,
Secure Keyboard Entry, low memory — and fixes its weaknesses: invisible features, no command awareness, no command palette, and
no trust signals around `sudo` and SSH. Liquid Glass is used **only on controls**; the terminal grid is opaque content.

> **Status: scaffold.** Architecture, security plumbing, shell integration and UI structure are in place.
> It was written without a Mac toolchain available, so expect a round of compile fixes. Items to check first are listed
> under [Verify first](#verify-first).

## Build

```sh
brew install xcodegen
xcodegen generate              # → Glasshouse.xcodeproj (app + embedded agent login item)
open Glasshouse.xcodeproj      # set DEVELOPMENT_TEAM in project.yml first (needed for Secure Enclave keychain access)
swift test                     # wire-protocol, OSC parser and APCA tests (no UI, no signing needed)
```

Distribution: Developer ID + hardened runtime + notarization. **Not sandboxed, not Mac App Store**: a sandboxed terminal's
child shells inherit the sandbox.

## Layout

```
Package.swift                    GlasshouseKit — UI-free libraries
Sources/
  TerminalCore/                  TerminalEngine protocol, SwiftTerm 1.x adapter, PTY env, termios password detection
  ShellIntegration/              OSC 133/7/9/777 scanner, CommandTimeline, zsh/bash/fish injection (no dotfile edits)
  GlasshouseSecurity/            Foreground-process inspection, Secure Keyboard Entry, Touch ID sudo + native SE SSH setup
  SSHAgentCore/                  RFC 9987 agent: wire format, Secure Enclave key store, BSD-socket server, peer identity
  DesignSystem/                  glassChrome() + accessible fallbacks, APCA contrast floor, TrustCapsule, tokens
Apps/
  Glasshouse/                    SwiftUI app: workspace, tabs, pane overlays, palette, approval card, settings
  GlasshouseAgent/               LSUIElement login item serving Secure Enclave keys (opt-in)
Tests/                           swift-testing suites
project.yml                      XcodeGen spec (bundle layout, entitlements, signing)
```

## SwiftTerm versions

| SwiftTerm | How to build | How the adapter gets output, OSC sequences and geometry |
|---|---|---|
| **1.99.1** (default; preview of 2.0) | nothing extra | `observeOscEvents`, `setProcessOutputHandler`, `terminalContentSnapshot(region:)` |
| **1.20.x** (legacy) | point `Package.swift` at upstream 1.20.x and enable the `SwiftTerm1` trait | overrides `dataReceived(slice:)` and reads `getTerminal().buffer` |

The default build uses [a fork](https://github.com/jasonkolodziej/SwiftTerm/tree/fix/teardown-qos-1.99.1): 1.99.1
plus one fix that runs process teardown at the I/O threads' priority, so closing a tab doesn't cause a priority inversion.
The fix has been sent upstream; switch back to `migueldeicaza/SwiftTerm` once a release includes it.

1.99 rebuilt SwiftTerm's input/output pipeline, so its API changed:
- `getTerminal()` and the public `terminal` property are gone.
- Output now goes to an internal adapter. `dataReceived(slice:)` can still be overridden, but it is **never called**, so an override would compile and silently do nothing.


## Layer model

| Layer | What | Material |
|---|---|---|
| L1 content | terminal grid, command gutter, sticky header | **opaque** (translucency is opt-in, Regular glass only, forced off by Reduce Transparency / Increase Contrast) |
| L2 controls | toolbar + tab strip, sidebar, find bar | system Liquid Glass |
| L3 transient | command palette, Touch ID approval card | glass + dimming via `glassChrome(.transient)` |
| L4 trusted | TrustCapsule, card header | filled capsule; driven **only** by kernel state (termios, process table) |

## Touch ID

**sudo:** Apple's `pam_tid` via `/etc/pam.d/sudo_local`. Glasshouse ships no PAM module and no privileged helper.
*Settings → Security → Set Up* opens a tab that shows the hardened script before it runs. The script:
- uses `sufficient`, so the password stays the fallback;
- if pam_reattach is needed, installs a root-owned copy in `/usr/local/lib/pam`;
- sets `timestamp_timeout=0`, so one tap approves exactly one command.

While `sudo` is in a tab's foreground process group, the **approval card** shows the command, user, host and tab, all read from the kernel. If the tab isn't visible, Glasshouse posts a notification instead.

**SSH:**
1. **Built-in (default).** `sc_auth create-ctk-identity -k p-256-ne -t bio` creates the key and `/usr/lib/ssh-keychain.dylib` is the provider. No Glasshouse code is in the signing path. Child shells get `SSH_SK_PROVIDER` set.
2. **Glasshouse Agent (opt-in).**
   - Uses Secure Enclave keys whose Touch ID prompt names the destination and the requesting process.
   - Supports per-host rules, and refuses forwarded requests unless a host's rule allows them.
   - Each host opts in with `IdentityAgent "~/.glasshouse/agent.sock"`. `SSH_AUTH_SOCK` is never set globally.

## Design critique → code

| Critique item | Where it landed |
|---|---|
| U1 ⌘K collision | `GlasshouseCommands`: ⌘K = clear (Terminal.app), hosts = ⇧⌘O, palette = ⇧⌘P |
| U2 card placement | `TerminalPane`: card anchored bottom, by the cursor, with a "Waiting for Touch ID…" phase |
| U3 hidden pane | `Notifier.approvalWaiting` when the pane isn't visible and key |
| U4 sticky header occlusion | shown only when the viewport is scrolled above the cursor row; never in the alternate screen |
| U5 gutter over text | 12 pt reserved inset (`GH.Size.gutterWidth`), 16 pt hit areas, hover popover |
| U6 sidebar default | collapsed; ⌃⌘S |
| U7 focus theft | VoiceOver announcement, no focus move |
| One trust signal | `TrustCapsule` — 3 states, filled + tinted; tab titles plain, sanitised |
| Contrast floor | `ContrastPolicy.standard` = Lc 75 / 60; Increase Contrast = 90 / 75 |

## Verify first

These were not checked against source or on a Mac. Each one is a quick prototype, worth doing before any UI polish.

1. **SwiftTerm version.** The adapter was checked against SwiftTerm's source at tags v1.20.0 and v1.99.1, but it hasn't been compiled. Default builds use **1.99.1** (from the fork above). To build against 1.20.x, point `Package.swift` at upstream 1.20.x and enable the `SwiftTerm1` trait. **1.99.0 is not supported.** Two things to watch:
   - SwiftTerm 1.99 is a preview of 2.0, so later 1.99.x releases may change its API again.
   - On 1.20, the cursor row is only accurate while the view is scrolled to the bottom (`yBase` isn't public), and `getText`'s row coordinates still need checking.
2. **`KERN_PROCARGS2` on setuid-root `sudo`.** If the kernel refuses it, the card falls back to the shell-reported command and labels it "reported by shell".
3. **Reading the PTY master on macOS**: termios (`tcgetattr`) must show the slave side's ECHO/ICANON state, and `tcgetpgrp` must return the slave's foreground process group.
4. **`LOCAL_PEERTOKEN` = 0x006 and `SOL_LOCAL` = 0** in the macOS 26 SDK (`PeerIdentity.swift`).
5. **macOS 26 SwiftUI APIs used:** `ToolbarSpacer` and `sharedBackgroundVisibility`, and `Glass.regular.tint(_:)`.
6. **Native SE SSH quirks:** `ssh-keygen -K` prompts for a PIN (leave it blank); duplicate stub file names when you have several keys (FB21992630).

## Not yet built (TODO markers in code)

- **Agent:** verifying the `session-bind` signature. The host key is display-only until this is done.
- **UI:**
  - Find bar search wiring and real scrollback clear.
  - Splits; session restore; AppleScript dictionary; App Intents.
  - Paste-confirmation sheet; OSC 52 read prompt.
- **Engine:** a libghostty adapter behind `TerminalEngine`, once libghostty ships a stable C API.
