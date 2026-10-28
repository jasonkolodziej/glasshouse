# Build Glasshouse: glass controls, opaque terminal

**Terminal.app on macOS Tahoe 26.x changed how it looks but not how it works, and its biggest failures are in correctness and discoverability.** Tahoe added 24-bit color, Liquid Glass chrome and the Clear Light/Clear Dark themes ([MacRumors](https://www.macrumors.com/2025/06/16/apples-terminal-app-macos-tahoe/)). The emulator underneath still drops the first character of OSC payloads, prints DCS/APC data to the screen, identifies itself as a vanilla VT100, has no kitty keyboard protocol, and mis-measures emoji width ([D'Amore](https://scour.ing/@abnv/p/https://garrett.damore.org/2025/12/macos-terminal-still-missing-mark-apple.html); [ucs-detect](https://ucs-detect.readthedocs.io/sw_results/appleterminal.html)). It also has no independent split panes, quick terminal, command palette or tmux integration. The proposed replacement, **"Glasshouse"**, is a Swift app that keeps the terminal grid opaque because it is *content*. It uses Liquid Glass only on the controls layer, which is how Apple's WWDC25 design sessions say the material should be used ([WWDC25 #219](https://developer.apple.com/videos/play/wwdc2025/219/)). It gets its modern features from OSC 133 shell integration drawn as overlays on the grid, the way VS Code and Ghostty do it, and leaves the user's line editor alone instead of replacing it the way Warp does. Glasshouse does not handle secrets itself. Touch ID for sudo is handled by Apple's `pam_tid` through `/etc/pam.d/sudo_local`, and SSH uses non-exportable Secure Enclave keys through `sc_auth` and `/usr/lib/ssh-keychain.dylib`. Glasshouse adds what neither native path has: a **trusted-chrome card**, drawn outside the grid, that shows the exact command, user, host and requesting tab. It builds that card from the kernel's process table and PTY `termios` state, not from bytes that a program could forge. An optional Secure Enclave ssh-agent login item can put the host and requesting process into the Touch ID prompt itself. The recommended engine is SwiftTerm, pinned to the 1.x line (1.20.x) behind a `TerminalEngine` protocol so that libghostty can replace it once it ships a tagged C API. The app ships unsandboxed with Developer ID signing, hardened runtime and notarization, because a sandboxed terminal cannot run a usable shell ([Apple DTS](https://developer.apple.com/forums/thread/685544)).

> **Reading guide and verification legend.** Research date: 2026-10-28. macOS 27 shipped on 2026-09-14, but no source reports changes to Terminal.app in it, and Apple's Terminal User Guide is version 2.15 for both macOS 26 and 27 ([blakecrosley.com](https://blakecrosley.com/blog/macos-27-release-notes); [Apple](https://support.apple.com/guide/terminal/use-marks-and-bookmarks-trml135fbc26/2.15/mac/27)). Tahoe-era findings therefore still apply. Labels used below: **[VERIFIED-DOC]** means the API signature was read in Apple documentation. **[VERIFIED-WWDC]** means it appears in a WWDC25 transcript. **[COMMUNITY]** means it comes from third-party code or a reference, not checked against the SDK. **[UNVERIFIED]** means it was not confirmed this session and must be checked in the Xcode 26 SDK before use. **[CONFLICT]** means the sources disagree; the disagreement is explained where it appears.

## Terminal.app's Tahoe refresh changed the paint, not the engine

### Functional gaps: capability-by-capability

The table compares Terminal.app on Tahoe with the best current implementation of each capability, and gives the Glasshouse v1 decision. "Engine-dependent" means the feature comes from the terminal engine (SwiftTerm in v1), not from Glasshouse's own code.

| Capability | Terminal.app (Tahoe 26.x) | Best-in-class | Glasshouse proposal |
|---|---|---|---|
| 24-bit color | Yes, new in 26 ([MacRumors](https://www.macrumors.com/2025/06/16/apples-terminal-app-macos-tahoe/)), detected only via `COLORTERM` ([ucs-detect](https://ucs-detect.readthedocs.io/sw_results/appleterminal.html)) | Universal | Export `COLORTERM=truecolor`; also answer DECRQSS/XTGETTCAP queries (engine-dependent) |
| Escape-sequence correctness | OSC drops its first character; DCS/APC/SOS/PM printed to screen; stray "p" after DECRQM; CSI intermediates mis-parsed ([D'Amore](https://scour.ing/@abnv/p/https://garrett.damore.org/2025/12/macos-terminal-still-missing-mark-apple.html)) | Ghostty 1.3: fuzzed parser, about 10 crashes fixed ([Ghostty 1.3.0](https://ghostty.org/docs/install/release-notes/1-3-0)) | Engine must silently discard unknown strings; fuzz the PTY-to-engine path in CI |
| Capability reporting (DA, XTVERSION, DECRQM, XTGETTCAP) | Reports "vanilla VT100"; XTVERSION returns nothing; XTGETTCAP queries printed as text ([ucs-detect](https://ucs-detect.readthedocs.io/sw_results/appleterminal.html)) | Ghostty | Answer XTVERSION with `Glasshouse <ver>`; XTGETTCAP arrives with SwiftTerm 1.99 ([SwiftTerm releases](https://github.1485827954.workers.dev/migueldeicaza/SwiftTerm/releases)), so it is a v1.1 item if v1 stays on 1.20 |
| Grapheme/emoji width | ZWJ 0%, VS16 50%; flags measured as 1 cell but drawn as 2 ([ucs-detect](https://ucs-detect.readthedocs.io/sw_results/appleterminal.html)) | Ghostty 93.2% scaled ucs-detect ([ucs-detect Ghostty](https://ucs-detect.readthedocs.io/_sources/sw_results/ghostty.rst.txt)) | Mode 2027 grapheme clustering is the goal; run ucs-detect as a release gate |
| Kitty keyboard protocol | No; cannot tell Ctrl-I from Tab ([D'Amore](https://scour.ing/@abnv/p/https://garrett.damore.org/2025/12/macos-terminal-still-missing-mark-apple.html)) | Ghostty, Kitty, Rio with all flags ([ucs-detect Rio](https://ucs-detect.readthedocs.io/sw_results/rio.html)) | Required; SwiftTerm 1.13 shipped kitty-keyboard fixes ([SwiftTerm releases](https://github.1485827954.workers.dev/migueldeicaza/SwiftTerm/releases)) |
| Synchronized output (DEC 2026) | Not reported; ucs-detect lists no DEC modes (**unverified**) | Ghostty, Kitty, WezTerm, Alacritty ([FrankenTUI](https://frankentui-docs.vercel.app/reference/terminal-compatibility)) | Required (engine-dependent) |
| Inline images | **[CONFLICT]** none per ucs-detect; "supported" per terminfo.dev (see callout) | Kitty/Ghostty (kitty), WezTerm (kitty + Sixel + OSC 1337) | SwiftTerm provides Kitty, Sixel and iTerm2 images ([SwiftTerm](https://github.com/migueldeicaza/SwiftTerm)) |
| GPU rendering / throughput | CoreText on CPU; Ghostty claims about 2× Terminal.app throughput ([Ghostty perf](https://www.mintlify.com/ghostty-org/ghostty/features/performance), vendor claim) | Ghostty (direct Metal, separate IO thread, renders only on change) | SwiftTerm Metal renderer; PTY read on a dedicated queue; render only when content changes |
| Independent split panes | No: ⌘D shows two views of *one* session ([Apple Discussions](https://discussions.apple.com/thread/250325189)) | iTerm2, Ghostty, Kitty, WezTerm, Rio | Real panes, each with its own PTY; drag to reorder like Ghostty 1.3 ([AlternativeTo](https://alternativeto.net/news/2026/3/ghostty-1-3-adds-scrollback-search-native-scrollbars-and-clickable-cursor-in-shell-prompts)) |
| Hotkey / quick terminal | No | iTerm2 Hotkey Window ([iTerm2](https://iterm2.com/features.html)); Ghostty quick terminal ([Ghostty 1.2.0](https://ghostty.org/docs/install/release-notes/1-2-0)) | Quick terminal as a system-wide hotkey panel |
| tmux integration | No | iTerm2 `tmux -CC` | Deferred to v2; v1 makes sure tmux works correctly as a plain client |
| Prompt marks | **Yes**: automatic marks; ⌘↑/⌘↓; ⇧⌘A selects output ([Apple](https://support.apple.com/guide/terminal/use-marks-and-bookmarks-trml135fbc26/2.15/mac/27)) | VS Code / Ghostty OSC 133 with exit status, duration and rerun ([VS Code](https://code.visualstudio.com/docs/terminal/shell-integration)) | Keep Terminal's shortcuts; upgrade the marks to OSC 133 blocks |
| Scrollback search | Basic Find | Ghostty 1.3: highlight-all, ⌘G navigation, draggable bar ([Ghostty 1.3.0](https://ghostty.org/docs/install/release-notes/1-3-0)); iTerm2 global regex | Glass find bar with regex, match count, and search across all tabs |
| Command palette | No | Ghostty 1.2 (includes actions that have no keybinding) ([Ghostty 1.2.0](https://ghostty.org/docs/install/release-notes/1-2-0)) | ⇧⌘P palette over every action, host, session and recent command |
| Config | GUI-only, profile editor with six panes ([Apple](https://support.apple.com/en-al/guide/terminal/trml107/2.15/mac/26)) | Ghostty file config; WezTerm Lua with hot reload | TOML file with live reload; the GUI edits the same file |
| SSH host management | "New Remote Connection" dialog with a bare address field ([Apple](https://support.apple.com/lt-lt/guide/terminal/trml1018/mac)) | Wave connection manager ([waveterm.dev](https://www.waveterm.dev/)); Ghostty terminfo copy to remote hosts | Sidebar built from `~/.ssh/config`, with a badge showing each host's key type |
| Long-command notifications | No | Ghostty `notify-on-command-finish`, 5 s default threshold ([Ghostty 1.3.0](https://ghostty.org/docs/install/release-notes/1-3-0)) | Notify only when the pane is unfocused and the command exceeds the threshold |
| Paste safety | Not documented | Ghostty replaces control characters in pastes (CVE-2026-26982) ([Ghostty 1.3.0](https://ghostty.org/docs/install/release-notes/1-3-0)) | Read-only confirmation sheet for multi-line pastes or pastes containing control characters |
| AI | No | Warp (requires an account for some features), iTerm2 3.7, Wave | Out of scope for v1; if added later, it must be opt-in and work on-device or offline |

> **[CONFLICT] Inline images in Terminal.app.** terminfo.dev (tested 2026-05-19, Terminal.app build not stated) marks Kitty graphics, Sixel and OSC 1337 as **supported** and scores Terminal.app at 84% ([terminfo.dev](https://terminfo-dev.pages.dev/terminals/terminal-app)). ucs-detect reports 0% graphics support for Apple_Terminal 465 ([ucs-detect](https://ucs-detect.readthedocs.io/sw_results/appleterminal.html)), and so do D'Amore and every comparison matrix found. terminfo.dev's own prose also contradicts its table. The most likely explanation is that terminfo.dev records that the parser *accepts* a sequence, not that anything is drawn. This report treats Terminal.app as **having no inline images**. Confirm with a `kitty +kitten icat` test before citing either number externally.

> **[CONFLICT] Latency figures.** One 2026 roundup gives Ghostty about 2 ms input latency ([dev.to](https://dev.to/vibehackers/best-terminal-for-mac-in-2026-ghostty-kitty-wezterm-alacritty-warp-more-4pe6), methodology not shown). A Typometer table gives about 13 ms ([cboone](https://github.fleetingtimer.workers.dev/cboone/reports/blob/main/terminal/macos-terminal-emulators.md)). Neither tested Terminal.app. Glasshouse should measure its own latency with Typometer on the target hardware and should not quote either figure.

### UX/UI design problems: features exist but nothing on screen reveals them

Terminal.app's main design failure is **invisible capability**. It automatically marks prompt lines whenever you press Return, ⌃C or ⌃D. It can jump between marks, select a command's output, clear to a mark, and keep named bookmarks ([Apple](https://support.apple.com/guide/terminal/use-marks-and-bookmarks-trml135fbc26/2.15/mac/27)). None of this appears anywhere in the window: there are no gutter dots and no scrollbar ticks, so even an enthusiast write-up has to introduce these features to readers ([Atomic Object](https://spin.atomicobject.com/use-marks-apple-terminal/), 2021, older). The marks are also heuristic. A mark records that Return was pressed, so it cannot carry an exit status, a duration or the working directory. As a result there is no "failed command" signal, no way to rerun a command, and no notification when something finishes. The same problem affects security. Secure Keyboard Entry is a checkmark in the app menu that applies to the whole app, not to a particular prompt ([Pareto Security](https://paretosecurity.com/auditor/checks/terminal-secure-entry)), so it says nothing about whether *this* prompt is protected.

The **settings design mixes looks with low-level behavior**. Changing a profile means working through six sub-panes. Terminfo, bell behavior and encodings are under "Advanced", Option-as-Meta is under "Keyboard", and "Restore Default Profiles" overwrites built-in profiles that the user has customized, which Apple's own help warns about ([Apple](https://support.apple.com/en-al/guide/terminal/trml107/2.15/mac/26)). Critics summarize Terminal.app as "tabs and that's about it": tabs can't be colored or grouped, URLs aren't highlighted, and paste history is limited ([How-To Geek via Yahoo](https://tech.yahoo.com/apps/articles/ditched-macs-default-terminal-app-140012891.html), 2024, older). The "New Remote Connection" dialog has no keyboard shortcut and a bare address field ([Apple](https://support.apple.com/lt-lt/guide/terminal/trml1018/mac)). It does not appear to read `~/.ssh/config`. That last point is inferred from the documented UI and is **unverified**. Parser bugs also look like design bugs to users. Text like "omething" or a stray "p" appearing on screen makes the app look broken, and no amount of visual polish fixes that ([D'Amore](https://scour.ing/@abnv/p/https://garrett.damore.org/2025/12/macos-terminal-still-missing-mark-apple.html)). Even the Tahoe visual refresh is unreliable: "Dynamic foreground colors" does not work with the new Clear profiles, and the WWDC "Powerline support" claim turned out to mean only that patched fonts render ([Apple Discussions](https://discussions.apple.com/thread/256241091)).

> **[CONFLICT] Bookmark shortcut.** Apple's macOS 26 shortcut guide lists Bookmark as ⌥⌘U ([Apple](https://support.apple.com/guide/terminal/trmlshtcts)). The older Atomic Object write-up gives ⇧⌘M ([Atomic Object](https://spin.atomicobject.com/use-marks-apple-terminal/)). Glasshouse should copy Apple's current table, which is what users' muscle memory will match.

### Five strengths Glasshouse must not lose

| Strength | Evidence | Glasshouse requirement (ship-blocking) |
|---|---|---|
| **VoiceOver / AX** | Ghostty's author says Terminal.app and iTerm2 "do it better than me", and that most GPU terminals don't use the system accessibility APIs "AT ALL" ([HN](https://hn-svelte.vercel.app/item/48002938)) | Expose an NSAccessibility text area with line navigation and announcements of new output, matching Terminal.app. Then go further: expose OSC 133 blocks as AX groups (command, output, exit status) |
| **Prompt marks** | ⌘U mark, ⌘↑/⌘↓ jump, ⇧⌘A select output, ⌘L clear to mark ([Apple](https://support.apple.com/guide/terminal/trmlshtcts)) | Keep the same shortcuts. Fall back to Return-based marks when shell integration is absent |
| **Low memory, instant start** | About 40 MB with instant startup, vs iTerm2 at about 180 MB+ and Warp at 100–600 MB ([cboone](https://github.fleetingtimer.workers.dev/cboone/reports/blob/main/terminal/macos-terminal-emulators.md)) | Budget: ≤ 60 MB idle with one tab (a target, not yet measured for SwiftTerm). No web views, and no Electron-style runtime |
| **Secure Keyboard Entry** | Menu toggle, and a CIS benchmark control ([Apple](https://support.apple.com/en-ca/guide/terminal/use-secure-keyboard-entry-trml109/2.15/mac/26); [Tenable CIS](https://tenable.com/audits/items/CIS_Apple_macOS_13.0_Ventura_v3.0.0_L1.audit:9205b5932881085ba15dc3d4ebcd5301)) | Keep the menu toggle and allow it to be enforced by an MDM-managed preference. Add per-prompt automatic engagement (described under the security design) |
| **AppleScript / system integration** | Ghostty added AppleScript in 1.3 and Shortcuts in 1.2 ([AlternativeTo](https://alternativeto.net/news/2026/3/ghostty-1-3-adds-scrollback-search-native-scrollbars-and-clickable-cursor-in-shell-prompts); [Ghostty 1.2.0](https://ghostty.org/docs/install/release-notes/1-2-0)) | Ship an AppleScript dictionary covering Terminal's `do script`, windows and tabs, plus App Intents, a Services menu item, "New Glasshouse at Folder", `.terminal` profile import, and state restoration. Terminal.app's own dictionary was not retrieved this session, so audit for parity against `sdef /System/Applications/Utilities/Terminal.app` |

## Glasshouse keeps the grid opaque and uses glass only on the controls

### Layer model: why glass never sits under text

Apple's rule is direct: Liquid Glass "is best reserved for the navigation layer that floats above the content of your app". In Apple's example, making a table view glass "would make it compete with other elements and muddy the hierarchy" ([WWDC25 #219](https://developer.apple.com/videos/play/wwdc2025/219/)). A terminal grid is dense monospace content with ANSI color, which is exactly that case. Apple also says the `.clear` variant "does not have adaptive behaviors" and needs a dimming layer, or "legibility gets noticeably worse". It should be used only over media-rich content with bold, bright foreground elements ([WWDC25 #219](https://developer.apple.com/videos/play/wwdc2025/219/)). Terminal text fails all three of those conditions. Apple's material is also more than frosted glass. Glassmorphism is a uniform `backdrop-filter` blur that "nothing about... reacts to content or movement", whereas Liquid Glass bends the content behind it, moves its specular highlights, and changes its own translucency depending on how bright the background is ([DesignMonks](https://www.designmonks.co/blog/liquid-glass-vs-glassmorphism)). It also responds automatically to Reduce Transparency, Increase Contrast and Reduce Motion ([WWDC25 #219](https://developer.apple.com/videos/play/wwdc2025/219/)). Glasshouse should therefore never draw its own blur. It gets glass from `NSToolbar`, `NSSplitViewController`, `NSGlassEffectView` and SwiftUI `.glassEffect`, and its designers mock up against Apple's real components. A "blur 20 + 10% white" Figma layer is not equivalent.

| Layer | Contents | Material | Rule enforced |
|---|---|---|---|
| L0 Window | Window frame, corner radius | System | Use `layoutGuide(for: .safeArea(cornerAdaptation: .horizontal))` so the larger Tahoe corners don't clip content ([WWDC25 #310](https://developer.apple.com/videos/play/wwdc2025/310/)) |
| L1 Content | Terminal grid, scrollbar gutter, sticky command header, selection, inline images | **Opaque.** `CAMetalLayer.isOpaque = true` | No glass on this layer and no glass behind it. Glass and text may only overlap while a transient panel is open |
| L2 Controls | Toolbar and tab strip, sessions/hosts sidebar, find bar | **Regular** Liquid Glass from system containers | Glass is never stacked on glass. Rows inside glass use fills and vibrancy ([WWDC25 #219](https://developer.apple.com/videos/play/wwdc2025/219/)) |
| L3 Transient | Command palette, Touch ID context card, paste-confirmation sheet | Regular glass + **dimming of L1** | These interrupt the user's task, so they pair glass with dimming ([WWDC25 #356](https://developer.apple.com/videos/play/wwdc2025/356/)) |
| L4 Trusted chrome | Lock indicator, approval card header, origin badge | Glass-free text inside L2/L3 regions, which program output cannot reach | Nothing a program writes can ever render here |

> **Opt-in translucent grid.** Ghostty and Rio both offer macOS 26 glass behind the grid, and both make it **opt-in**. Ghostty applies blur only when `background-opacity < 1` ([Ghostty config](https://ghostty.org/docs/config/reference)). Rio places its non-opaque `CAMetalLayer` inside `NSGlassEffectView.contentView`, tints it with the theme's background color, and matches `cornerRadius` to the window ([rio-window glass.rs](https://docs.rs/crate/rio-window/0.5.13/source/src/platform_impl/macos/glass.rs)). Glasshouse can offer the same mode, with four restrictions. It always uses Regular glass, never Clear. Glyph opacity stays at 1. The APCA contrast floor below is still enforced. The mode is forced off whenever Reduce Transparency or Increase Contrast is on. A non-opaque Metal layer costs extra compositing on every frame. No measurements of that cost were found, so treat it as **[UNVERIFIED]** and profile it in Instruments.

### Screen-region and layout specification

Dimensions marked "proposed" are Glasshouse design decisions, not Apple metrics.

| Region | Layer | Implementation | Geometry (proposed unless cited) | Behavior | Accessibility fallback |
|---|---|---|---|---|---|
| Toolbar + tab strip | L2 | `NSToolbar` grouped items; `ToolbarSpacer(.fixed)` separates groups ([WWDC25 #323](https://developer.apple.com/videos/play/wwdc2025/323/)) | Standard toolbar height; `prefersCompactControlSizeMetrics = true` for density ([WWDC25 #310](https://developer.apple.com/videos/play/wwdc2025/310/)) | Tabs form one glass group. The title and status text are unbordered (`isBordered = false`), because non-interactive items "should avoid the glass material" ([WWDC25 #310](https://developer.apple.com/videos/play/wwdc2025/310/)). Only the active tab gets an accent tint | System glass turns frostier or opaque automatically |
| Trusted-status slot | L4 in L2 | Fixed toolbar item at a position the tab title cannot occupy | 1 SF Symbol plus a short label (proposed) | Shows 🔒 "Secure input" / "sudo · Touch ID" / "ssh · SE key". Driven only by process and termios state | VoiceOver announces any change of state |
| Sidebar: Sessions / SSH Hosts | L2 | `NSSplitViewController` sidebar (floating glass); remove any `NSVisualEffectView` ([WWDC25 #310](https://developer.apple.com/videos/play/wwdc2025/310/)) | Collapsible; 220–280 pt (proposed) | Sections: Open Sessions (tab, cwd, running command), Hosts (from `ssh -G`), Workspaces | Opaque sidebar under Reduce Transparency |
| Terminal grid | L1 | `NSView` hosting `CAMetalLayer`, wrapped in `NSViewRepresentable` with an empty `updateNSView` ([Ghostty early source](https://git.uoc.run.place/Applied-Software/ghostty/raw/commit/6ceb5b847b09339373d94ba2bab5fe36d9bd048b/macos/Sources/TerminalSurfaceView.swift)) | Inset by the toolbar safe area so there is no steady-state overlap with glass | Opaque; render only when content changes; the APCA floor is applied to cell colors | Always opaque under Reduce Transparency and Increase Contrast |
| Top edge separator | L1 | Hand-drawn hairline or hard edge. The grid is not an `NSScrollView`, so the system scroll-edge effect is not applied automatically ([WWDC25 #310](https://developer.apple.com/videos/play/wwdc2025/310/)) | 1 px | Apple prefers the hard style on macOS for dense, interactive text ([WWDC25 #356](https://developer.apple.com/videos/play/wwdc2025/356/)) | Contrasting border under Increase Contrast |
| Scrollbar gutter with command marks | L1 | Overlay scrollbar (as in Ghostty 1.3) that takes no grid columns | 8–10 pt overlay track (proposed) | One tick per OSC 133 block: neutral for success, red for nonzero exit, outlined for a running command. Clicking a tick jumps to that prompt | Ticks also differ in shape, not only color. AX exposes them as a list of commands |
| Sticky command header | L1 (solid fill, **not glass**) | Pinned row at the top of the grid viewport | One cell row + 4 pt padding (proposed) | Shows the command whose output fills the top of the viewport (like Warp and VS Code); click to jump; hidden in the alternate screen until the user scrolls ([Warp](https://docs.warp.dev/terminal/blocks/sticky-command-header)) | Solid surface, so contrast is predictable |
| Find bar | L2 | Glass bar docked as a top accessory that pushes the grid down rather than floating over it | One control row | Regex, case, whole word, "All tabs", match count, ⌘G/⇧⌘G | Opaque fallback |
| Command palette | L3 | `NSGlassEffectView` with the palette view as `contentView` (never as a sibling behind it) ([WWDC25 #310](https://developer.apple.com/videos/play/wwdc2025/310/)) | Centered, 560–640 pt wide (proposed) | Dims L1; rows use fills; one search field for both actions and items ([Retool](https://retool.com/blog/designing-the-command-palette)) | Opaque panel; no morph animation under Reduce Motion |
| Touch ID context card | L3 + L4 | Glass card anchored to the requesting pane's top edge | 360–420 pt (proposed) | Dims *only* the requesting pane. Shows command, user, host, tab and source trust level | Opaque; VoiceOver reads the whole card first |
| Paste confirmation | L3 | Sheet with a read-only text view | Modal to the window | Shows exactly the bytes that will be sent, with control characters made visible | Standard sheet |

### Accessibility fallbacks and the contrast floor

System glass adapts on its own: Reduce Transparency "makes Liquid Glass frostier", and Increase Contrast "makes elements predominantly black or white" with a border ([WWDC25 #219](https://developer.apple.com/videos/play/wwdc2025/219/)). Glasshouse must not override those adaptations. Its job is to gate its *own* transparency on the same settings. The 26.1 Clear/Tinted switch helps less on Mac than people expect. Tinted reduces transparency only "a little" there ([Six Colors](https://sixcolors.com/post/2025/11/soaping-up-liquid-glass-less-transparency-more-contrast/)), and title-bar legibility was "unaffected" by it as of 26.5 ([Eclectic Light](https://eclecticlight.co/2026/05/31/last-week-on-my-mac-razzle-and-dazzle/)). No developer API exposing the Clear/Tinted choice was found. macOS 27 adds an opacity slider ([MacRumors](https://www.macrumors.com/2026/06/09/macos-golden-gate-liquid-glass/)), but no developer API changes were reported with it ([Gizmodo](https://gizmodo.com/apple-fixes-the-illegible-liquid-glass-elements-on-macos-2000768705)). Glasshouse should therefore inherit those settings through the system containers and not try to read them.

| User setting | System glass (L2/L3) | Glasshouse-owned surfaces | Detection |
|---|---|---|---|
| Reduce Transparency | Frostier, handled by the system | Translucent-grid mode forced off; custom glass becomes `.identity` | SwiftUI `\.accessibilityReduceTransparency` **[COMMUNITY]** ([LiquidGlassReference](https://github.com/conorluddy/LiquidGlassReference)); AppKit `NSWorkspace.shared.accessibilityDisplayShouldReduceTransparency` **[UNVERIFIED]** |
| Increase Contrast | Black/white with borders; also forces Reduce Transparency on ([Six Colors](https://sixcolors.com/post/2025/11/soaping-up-liquid-glass-less-transparency-more-contrast/)) | Hairlines around the gutter and sticky header; APCA floor raised to Lc 90 | `accessibilityDisplayShouldIncreaseContrast` **[UNVERIFIED]**; observe `NSWorkspace.accessibilityDisplayOptionsDidChangeNotification` **[UNVERIFIED]** |
| Reduce Motion | Weaker effects, no elastic animation ([WWDC25 #219](https://developer.apple.com/videos/play/wwdc2025/219/)) | `glassEffectTransition(.identity)` **[COMMUNITY]**; no palette morph; cursor blink off by default | `\.accessibilityReduceMotion` |
| Clear/Tinted (26.1+), opacity slider (27) | Applied by the system | Nothing to do | No API found |

**APCA auto-contrast floor.** Zed and Ghostty both adjust ANSI colors automatically so they stay readable. Zed uses APCA rather than WCAG because WCAG 2 is unreliable on dark backgrounds. It changes lightness first, then saturation, and falls back to black or white as a last resort ([Zed commit](https://git.secluded.site/zed/commit/9b7632d5f6621e2fd188b1b2b04b386a9e7b7746)). APCA sets Lc 75 as the minimum for body text and prefers Lc 90 ([Myndex APCA](https://github.com/Myndex/apca-introduction)). Terminal text at 11–14 pt is smaller than APCA's web size buckets. Glasshouse therefore uses two thresholds. The **default foreground must reach at least Lc 75** against the effective cell background, and **ANSI palette colors must reach at least Lc 45**. Both are adjustable, and Increase Contrast raises them to Lc 90 and Lc 60. With an opaque grid the effective background is known, so the check is deterministic. With the translucent mode on it is not, which is one more reason that mode stays opt-in.

> **[CONFLICT] Zed's default.** Zed's `default.json` sets `minimum_contrast` to 45, but one doc comment in the same commit says 0 ([Zed commit](https://git.secluded.site/zed/commit/9b7632d5f6621e2fd188b1b2b04b386a9e7b7746)). Glasshouse's Lc 45 palette floor follows the `default.json` value.

### Shell integration as overlays, not a replacement line editor

Terminals build command blocks in one of two ways. Warp replaces the line editor and keeps a separate grid for each command ([Warp blog](https://www.warp.dev/blog/how-warp-works)). That broke zsh vi-mode and custom completions for users ([willhbr.net](https://willhbr.net/2023/08/02/warp-terminal/)), and reviewers found the blocks took up too much space ([matduggan.com](https://matduggan.com/Warp-terminal-emulator-review/)). VS Code, Ghostty and iTerm2 keep the normal grid and the user's own shell, and draw decorations on top from OSC 133 markers: A marks the prompt, B the input, C the output, and D the end of the command with its exit code ([terminfo.dev OSC 133](https://terminfo-dev.pages.dev/extensions/osc-133-semantic-prompts)). Glasshouse takes the second approach. Every feature in the table below degrades to Terminal.app's behavior when the shell sends no markers.

| Feature | Signal | UI (layer) | Precedent |
|---|---|---|---|
| Command marks in the gutter | OSC 133 A/D + exit code | Tick per block, red if nonzero (L1) | VS Code overview-ruler marks ([VS Code](https://code.visualstudio.com/docs/terminal/shell-integration)) |
| Jump to prompt | Block index | ⌘↑/⌘↓; ⇧⌘↑/↓ extends the selection | Terminal.app marks; VS Code |
| Sticky command header | Block whose output starts above the viewport | Pinned row (L1) | Warp, VS Code sticky scroll |
| Copy output / copy command / rerun | C→D range; command text from preexec | Popover on gutter click; ⇧⌘A copies the last output | VS Code decoration menu |
| Finish notification | D received, duration ≥ threshold, pane not key | `UNUserNotificationCenter` banner titled with the tab name | Ghostty `notify-on-command-finish` ([Ghostty 1.3.0](https://ghostty.org/docs/install/release-notes/1-3-0)) |
| Progress | OSC 9;4 | Tab-strip progress ring | Ghostty 1.2 ([Ghostty 1.2.0](https://ghostty.org/docs/install/release-notes/1-2-0)) |
| cwd-aware links, new tab in same directory | OSC 7 | ⌘-click on paths; inherit cwd | Zed ([Zed docs](https://zed.dev/docs/terminal)) |
| Click to move the cursor in the prompt | OSC 133 `cl=` | Only for shells that support it (fish 4.1+, Nushell) | Ghostty 1.3 |
| AX command structure | Blocks | AX groups: "command, exit 1, 3.2 s" | Hashimoto's direction for Ghostty ([HN](https://hn-svelte.vercel.app/item/48002938)) |

Two parsing rules matter here. First, bare `OSC 9;text` is an iTerm2 notification while `OSC 9;4;…` is ConEmu progress, so the parser must tell them apart by whether the first parameter is numeric, as Ghostty does ([Ghostty 1.3.0](https://ghostty.org/docs/install/release-notes/1-3-0)). Second, OSC 133 command text arrives in-band from the shell, so the security UI treats it as **untrusted display data** (see below).

### Interaction flows

**Flow A: new SSH session from the sidebar.** (1) The user presses ⌘K, or opens Hosts in the sidebar. The host list comes from `ssh -G <alias>` run against each `Host` entry in `~/.ssh/config`, including files pulled in by `Include`, so the hostname, user, port and `IdentityFile` shown are what ssh will actually use. (2) Each host row has a badge for its credential type. **SE** means an `sk-ecdsa` stub with `SecurityKeyProvider /usr/lib/ssh-keychain.dylib`. **Agent** means Glasshouse's agent, set with `IdentityAgent`. **Other** covers everything else. (3) Pressing Return opens a new tab that runs `/usr/bin/ssh <alias>`. Glasshouse invokes the system ssh and does not reimplement it. Homebrew OpenSSH ignored `SSH_SK_PROVIDER` as of 2024 ([openssh-unix-dev](https://lists.mindrot.org/pipermail/openssh-unix-dev/2024-July/041451.html)). (4) When the Touch ID sheet appears, the trusted-status slot shows "ssh · SE key → prod-db-1 (from `ssh -G`)", and the context card names the tab. (5) After login, the tab title becomes `user@host`. Prompt-mark features continue only if the remote shell emits OSC 133, because Glasshouse does not install remote integration in v1.

**Flow B: sudo approval in trusted chrome.** (1) The user runs `sudo systemctl …` locally, or a script does. (2) The PTY monitor sees that the foreground process group changed (`tcgetpgrp(master)`), finds that the group contains `/usr/bin/sudo`, and reads that process's argv, real UID and target user from the kernel. (3) `pam_tid` shows Apple's Touch ID sheet, which does not display the command ([cc-sudo](https://github.com/yasyf/cc-sudo)). At the same moment Glasshouse slides out a glass card from the pane's top edge. The card shows **Command**: `systemctl restart nginx`; **Run as**: root; **Host**: this Mac; **From**: Tab 3 · ~/infra; **Source**: kernel process table. (4) If the user cancels the sheet, sudo falls back to its password prompt. The PTY's termios then switches to ECHO off with ICANON on. Glasshouse sees this, turns the status slot into 🔒 "Secure input", and enables Secure Event Input for the pane. (5) When sudo exits, both the card and the lock disappear. **A Touch ID sheet that appears without a Glasshouse card did not come from this terminal.** The onboarding flow teaches that rule once.

**Flow C: command palette.** ⇧⌘P opens one field that searches actions (including those with no keybinding), hosts, open sessions, recent commands (from OSC 133 history) and recent directories. A "Top result" row wins regardless of category ([Retool](https://retool.com/blog/designing-the-command-palette)), and every row shows its shortcut so users learn it ([Raycast](https://www.raycast.com/blog/a-fresh-look-and-feel)). Pressing ⌥-Return on a recent command inserts it without running it, as in VS Code's Run Recent Command ([VS Code](https://code.visualstudio.com/docs/terminal/shell-integration)). The palette stays open for multi-step flows: pick a host, then a pane layout, then open.

## Security design: secrets stay out of the byte stream and trust signals stay out of the grid

### sudo: Apple's pam_tid, with context Glasshouse cannot forge

Glasshouse ships **no PAM module and no privileged helper**. Touch ID for sudo is one file: copy `/etc/pam.d/sudo_local.template` to `sudo_local` and uncomment `auth sufficient pam_tid.so`. The file survives OS updates, because since Sonoma the stock `/etc/pam.d/sudo` begins with `auth include sudo_local` ([Six Colors](https://sixcolors.com/post/2023/08/in-macos-sonoma-touch-id-for-sudo-can-survive-updates/)). The flag must stay `sufficient`, never `required`, so that a cancelled Touch ID prompt falls back to the password and nobody gets locked out ([erikw](https://erikw.netlify.app/blog/tech/finger-of-god-using-sudo-with-touchid-on-macos)). Glasshouse's setup assistant also recommends `Defaults timestamp_timeout=0` in a `visudo`-validated drop-in. That way every tap authorizes exactly one command, which matches what the DISA STIG for macOS 26 already requires ([Tenable STIG](https://www.tenable.com/audits/items/DISA_STIG_Apple_macOS_26_Tahoe_v1r2.audit:d279fc6463a0338d7817fd9c85d3b6d1)). The assistant does this by **opening a visible tab containing the exact commands** for the user to read and run. It does not run hidden `AuthorizationExecuteWithPrivileges` calls. tmux users need `pam_reattach`, and the assistant must install it from a root-owned copy, never from `/opt/homebrew/lib/pam`. On Apple Silicon that Homebrew path is writable by the user, so any user-level process could plant code inside setuid sudo ([pam_reattach](https://www.github.com/fabianishere/pam_reattach)).

Glasshouse's added value is the context card, and its trust comes from **where the data comes from**. WezTerm and Ghostty detect password prompts from the PTY's termios state: ECHO off with ICANON on. The kernel sets that state, so program output cannot fake it ([WezTerm](https://wezterm.org/config/lua/config/detect_password_input.html); [Ghostty](https://git.uoc.run.place/Applied-Software/ghostty/commit/a513a02328dc2ab853724e76793532121045e81a)). Glasshouse adds a second kernel-derived signal: which processes are in the PTY's foreground process group. A `pam_tid` wait does not necessarily put the tty into no-echo mode, so termios alone would miss the Touch ID phase. Process membership covers it.

| Card field | Primary source (trusted) | Fallback (shown as "reported by shell") | Notes |
|---|---|---|---|
| Is this sudo? | `tcgetpgrp(masterFD)`, then `proc_listpids(PROC_PGRP_ONLY, pgid)`, then `proc_pidpath` == `/usr/bin/sudo` | none | **[UNVERIFIED]** that `proc_pidpath` succeeds on a setuid-root process owned by the same real UID |
| Command argv | `sysctl(KERN_PROCARGS2, pid)` | OSC 133 preexec command line | **[UNVERIFIED]** that `KERN_PROCARGS2` is readable for a setuid-root process; it may return EPERM. If so, use the fallback and switch the card to its lower-trust style |
| Target user | Parse `-u` from argv; default is root | — | |
| Host | Local, unless the foreground group's leader is `ssh`/`mosh` | — | Remote sudo through ssh uses the remote password; Touch ID cannot cross the network ([sudo-touchid #19](https://www.github.com/mattrajca/sudo-touchid/issues/19)) |
| Secret input active | termios ECHO off and ICANON on, polled on each keystroke and on a 100 ms timer while sudo or ssh is in the foreground (interval proposed) | — | Doesn't work through ssh or tmux, because those put the local tty in raw mode ([WezTerm](https://wezterm.org/config/lua/config/detect_password_input.html)) |

> **Callout: what Glasshouse cannot see.** Inside `ssh` or `tmux`, the local PTY is in raw mode, so a remote `[sudo] password:` prompt cannot be told apart from ordinary input. Ghostty documents the same limitation ([Ghostty](https://git.uoc.run.place/Applied-Software/ghostty/commit/a513a02328dc2ab853724e76793532121045e81a)). Glasshouse says this openly: while ssh is in the foreground, the status slot reads "remote · prompts not detectable", and the user can turn on Secure Keyboard Entry by hand. Glasshouse never guesses at password prompts by matching text on screen.

### SSH: native Secure Enclave path first, an optional agent for context

**Default path, with no Glasshouse code in the signing path.** `sc_auth create-ctk-identity -l ssh -k p-256-ne -t bio` creates a non-exportable P-256 key in the Secure Enclave that requires biometrics. `ssh-keygen -w /usr/lib/ssh-keychain.dylib -K` downloads a stub file that holds only a key handle. `SecurityKeyProvider /usr/lib/ssh-keychain.dylib` in each `Host` block makes the system OpenSSH use it as a resident `sk-ecdsa-sha2-nistp256@openssh.com` key ([arianvp gist](https://gist.github.com/arianvp/5f59f1783e3eaf1a2d4cd8e952bb4acf); [ewpratten](https://ewpratten.com/blog/ssh-secure-enclave/)). Glasshouse's Hosts view can run these three steps visibly in a tab, write a managed `Include ~/.ssh/config.d/glasshouse` block with `IdentitiesOnly yes`, `IdentityAgent none` and `ForwardAgent no`, and remind the user to register a recovery key, because Secure Enclave keys can't be backed up. Limits carried over from the earlier research: only P-256 works (`p-384-ne` is rejected), servers must accept `sk-` key types (OpenSSH ≥ 8.2), there is no attestation, and OpenSSH asks for a meaningless PIN, which `SSH_ASKPASS_REQUIRE=force SSH_ASKPASS=true` suppresses. As with sudo, Apple's Touch ID sheet for this path doesn't name the destination ([OpenSSH agent-restrict](https://www.openssh.org/agent-restrict.html)). Glasshouse's card adds it, using the same foreground-process method: argv of the local `ssh` process (same UID, so readable), resolved through `ssh -G`.

**Optional path: "Glasshouse Agent", off by default.** Some users want the host and the requesting process **inside the Touch ID prompt itself**, or per-host rules such as "always require a fresh tap for `prod-*`" or "refuse forwarded requests". For them, Glasshouse bundles a Secure Enclave ssh-agent as a login item. It keeps its own non-exportable keys (CryptoKit `SecureEnclave.P256.Signing.PrivateKey` with `[.privateKeyUsage, .userPresence]`, or `.biometryCurrentSet` in strict mode) and serves plain `ecdsa-sha2-nistp256`, which works with servers that reject `sk-` keys. It implements the RFC 9987 messages 11→12 and 13→14, answers FAILURE (5) to everything else, and answers EXTENSION_FAILURE (28) to unknown extensions ([RFC 9987](https://www.rfc-editor.org/rfc/rfc9987.xml)). It identifies the destination host from OpenSSH's `session-bind@openssh.com` extension, which carries the host key, session ID, server signature and an `is_forwarding` flag. The host key is used for **policy only after the agent has verified the signature**; before that it is display-only ([PROTOCOL.agent](https://fuchsia.googlesource.com/third_party/openssh-portable/+/refs/heads/main/PROTOCOL.agent)). It identifies the requesting process from `LOCAL_PEERTOKEN`, an audit token rather than a PID, passed to `SecCodeCopyGuestWithAttributes` ([Dev Forums 850777](https://developer.apple.com/forums/thread/850777)). The result goes into `LAContext.localizedReason`, for example "sign in to prod-db-1 with 'work' key (requested by ssh in Glasshouse, Tab 3)". Clients reach the agent through a per-host `IdentityAgent <socket>`. Glasshouse **never overwrites `SSH_AUTH_SOCK`** for the whole system or for all of its child shells ([Secretive APP_CONFIG](https://github.fontlos.workers.dev/maxgoedjen/secretive/blob/main/APP_CONFIG.md)).

> **[CONFLICT] Agent vs. login item packaging.** The engine notes suggest a bare helper executable in `Contents/MacOS/`, registered with `SMAppService.agent(plistName:)` through a `BundleProgram` plist ([Dev Forums 802443](https://developer.apple.com/forums/thread/802443)). The earlier Touch ID research instead puts the agent in an embedded `.app` registered with `SMAppService.loginItem(identifier:)`. Its reason is that the data-protection keychain needs a restricted `keychain-access-groups` entitlement authorized by a provisioning profile, and only an app-like bundle can embed one; without it, calls fail with `-34018` ([TN3137](https://developer.apple.com/documentation/technotes/tn3137-on-mac-keychains.md); [Dev Forums 759481](https://developer.apple.com/forums/thread/759481)). **This report follows the keychain constraint:** `GlasshouseAgent.app` is an LSUIElement app in `Contents/Library/LoginItems/`, registered with `loginItem(identifier:)`, and it shares a keychain access group with the main app. If a bare executable can carry a profile on Developer ID, the `agent(plistName:)` route also works; that is **[UNVERIFIED]**. Either way, a status of `.requiresApproval` must send the user to `SMAppService.openSystemSettingsLoginItems()` ([Apple SMAppService](https://developer.apple.com/documentation/servicemanagement/smappservice.md)).

| Agent hardening item | Setting |
|---|---|
| Sandbox | The **agent** is sandboxed (it spawns nothing). The main app is not |
| Socket | BSD `AF_UNIX`, not `NWListener`, because of the 104-byte `sun_path` limit ([Dev Forums 756756](https://developer.apple.com/forums/thread/756756)); `umask 077`; mode 0600; reject peers with a different UID via `getpeereid` |
| Parsing | Frames capped at 256 KiB; every string bounds-checked; optionally parse in a separate XPC service, as Secretive 3 does ([Secretive v3.0.0](https://newreleases.io/project/github/maxgoedjen/secretive/release/v3.0.0)) |
| Prompt cache | None. `touchIDAuthenticationAllowableReuseDuration` does not apply to keychain-gated operations ([Apple](https://developer.apple.com/documentation/localauthentication/lacontext/touchidauthenticationallowablereuseduration.md)) |
| Forwarding | Refuse when `session-bind` reports `is_forwarding = true`, unless the host's policy explicitly allows it |

### Secure Keyboard Entry that switches itself on and off

Apple's TN2150 sets the limits. `EnableSecureEventInput` is global, applying "whether that process is in the foreground or background". A process "should never enable secure event input for general text input or just enable this function for the life of the application", and every enable must be paired with a `DisableSecureEventInput` ([TN2150](https://developer.apple.com/library/mac/technotes/tn2150/_index.html)). Glasshouse's controller therefore keeps a **per-pane reference count**. It enables secure input only when the pane is key *and* its termios shows ECHO off with ICANON on. It disables secure input on resign-key, on the pane closing, when echo returns, and on app deactivation. The menu item "Secure Keyboard Entry (always)" stays available for CIS compliance and can be set by a managed preference. Its tooltip should repeat iTerm2's warning that secure input "may disable global hotkeys in other programs" ([iTerm2](https://iterm2.com/3.0/documentation-menu-items.html)) and the Ghostty note that it interferes with accessibility software ([Ghostty](https://git.uoc.run.place/Applied-Software/ghostty/commit/a513a02328dc2ab853724e76793532121045e81a)). Note that `IsSecureEventInputEnabled()` can report true because of *another* process. The indicator must reflect Glasshouse's own reference count, not that global value.

### Prompt-spoofing defenses

WallEscape (CVE-2024-28085) showed that escape sequences can draw a fake `[sudo] password` prompt, hide keystrokes, and recolor text so it blends in ([oss-security](https://openwall.com/lists/oss-security/2024/03/27/5)). The defense is architectural: **anything that signals trust is drawn in L4 from kernel state, and nothing a program writes can ever reach L4.**

| Threat | Defense |
|---|---|
| Fake password prompt drawn in the grid | Real prompts produce the L4 lock and card. Fakes cannot, because termios and process-group state can't be forged with bytes. Onboarding teaches "no lock, no password" |
| Malicious title (OSC 0/2) imitating a lock or "sudo" | Titles render only in the tab label, never in the trusted-status slot. Lock glyphs and selected SF Symbol code points are stripped from program-set titles |
| Notification spoofing (OSC 9/777/99) | Banners are always prefixed with the Glasshouse tab name, rate-limited, and never use security wording that Glasshouse itself uses |
| A Touch ID sheet from another app timed to match a terminal action | If no Glasshouse card appears, the request came from elsewhere. The card shows the requesting pid and path from the kernel |
| Paste injection | Unsafe control characters replaced and a read-only confirmation sheet shown (Ghostty's CVE-2026-26982 fix) ([Ghostty 1.3.0](https://ghostty.org/docs/install/release-notes/1-3-0)); bracketed paste on |
| Clipboard exfiltration (OSC 52 read) | Reading is denied by default. Each request triggers a per-host prompt with a "remember" option (Ghostty 1.2 precedent) ([Ghostty 1.2.0](https://ghostty.org/docs/install/release-notes/1-2-0)) |
| Spoofed OSC 133 command text in the card | Shell-reported text is shown only in the lower-trust style labeled "reported by shell" |
| Scripts opened from Finder | Confirm before running (Ghostty 1.2 precedent) |

## SwiftTerm 1.20 behind a protocol now, with libghostty as the planned replacement

### Engine decision

SwiftTerm is the cheapest engine to integrate, and it is already used in shipping apps. It is MIT-licensed pure Swift with an AppKit view, CoreGraphics and Metal renderers, Kitty, Sixel and iTerm2 images, BiDi, and OSC 133 since v1.17 ([SwiftTerm](https://github.com/migueldeicaza/SwiftTerm); [releases](https://github.1485827954.workers.dev/migueldeicaza/SwiftTerm/releases)). libghostty is faster and more proven in Ghostty, but its C API was still "work-in-progress" in Ghostty 1.3, which said "We aren't sure yet when we'll tag the first libghostty releases" ([Ghostty 1.3.0](https://ghostty.org/docs/install/release-notes/1-3-0)). Only `libghostty-vt`, which parses and holds state but does not render, has public C examples ([Ghostty examples](https://upd.dev/ghostty-org/ghostty/src/branch/main/example)). The third-party GhosttyKit SPM package exists, but it requires Zig and local patches to rebuild ([libghostty-spm](https://swiftpackageregistry.com/Lakr233/libghostty-spm)).

| Criterion | SwiftTerm 1.20.x | SwiftTerm 1.99.x (2.0 preview) | libghostty (GhosttyKit / internal API) | libghostty-vt + own renderer |
|---|---|---|---|---|
| API stability | Frozen 1.x line, maintained on the `v1.x` branch | Breaking changes landing; 1.99.1 released 2026-10-21 | Unofficial / internal | Pre-1.0, untagged ([libghostty-vt-sys](https://docs.rs/crate/libghostty-vt-sys/0.2.0/source/README.md)) |
| Notable features | OSC 133, BiDi, IME overlays, Metal | Adds new IO, "Kitty2", XTGETTCAP, bracketed paste | Full Ghostty feature set | Parser and state only |
| Toolchain | SPM | SPM | Zig, prebuilt XCFramework | Zig plus a Metal renderer you write yourself |
| Verdict | **v1 engine** | Spike on a branch; adopt when it reaches 2.0 | v2 candidate after the C API is tagged | Only if you want to own the whole stack |

> **Callout: pinning trap.** SwiftPM's default `from: "1.20.0"` means "up to the next major version", so it **resolves to 1.99.1**, the breaking 2.0 preview. The 1.20.0 release notes call it "one last release before we land the breaking changes" ([SwiftTerm releases](https://github.1485827954.workers.dev/migueldeicaza/SwiftTerm/releases)). Use `.upToNextMinor(from: "1.20.0")` or `exact:`. Ignore swiftpackageregistry.com's "latest 1.2.1", which is stale ([Swift Package Registry](https://swiftpackageregistry.com/migueldeicaza/SwiftTerm)).

### Module breakdown

The app is an Xcode project because SwiftPM alone cannot produce a signed `.app` with embedded login items, plists and entitlements. Most of the code lives in a local Swift package, `GlasshouseKit`, so it can be tested with Swift Testing in CI.

| Module (target) | Kind | Responsibility | Depends on | Concurrency |
|---|---|---|---|---|
| `Glasshouse` | App (Xcode) | Windows, `NSToolbar`, split view, menus, AppleScript `sdef`, App Intents, settings UI, onboarding | all below | `@MainActor` |
| `DesignSystem` | SPM | Glass wrappers (`GlassPanel`, `GlassCard`), layer tokens, accessibility environment, APCA contrast math, theme model | — | `@MainActor` views; pure functions for color |
| `TerminalCore` | SPM | `TerminalEngine` protocol; `SwiftTermEngine` adapter; `PTYSession` (spawn, read loop, resize, reap); environment builder; config model and file watcher | `CPTY`, SwiftTerm | PTY I/O in a dedicated actor or queue; render invalidation hops to main |
| `CPTY` | SPM C target | `openpty`, `login_tty`, `fork`/`execve` shim; `tcgetattr` and `tcgetpgrp` wrappers | libc | Async-signal-safe C only |
| `ShellIntegration` | SPM + resources | Bundled zsh/bash/fish scripts; injector; OSC 133/7/9/777/99 model; `CommandBlock` store; notifications | `TerminalCore` | actor `BlockStore` |
| `SecurityUX` | SPM | `TermiosWatcher`, `ForegroundProcessInspector`, `SecureInputController` (reference-counted), `TrustedChromeModel`, paste sanitizer, OSC 52 policy | `CPTY`, `TerminalCore` | actor per pane; `@MainActor` for the chrome model |
| `TouchIDSetup` | SPM | Read-only checks on `sudo_local`, sudoers drop-in, and `sc_auth list-ctk-identities`; generates the visible setup script | — | async |
| `SSHHosts` | SPM | `ssh -G` resolver, `Include`-aware config scan, managed `config.d/glasshouse` writer, host badges | — | async |
| `AgentKit` | SPM | RFC 9987 codec (pure, Linux-testable), `session-bind` parser and verifier, policy engine, BSD socket server | CryptoKit, Security | actor `Signer`; actor `AgentServer` |
| `GlasshouseAgent` | App (LSUIElement login item) | Owns the socket and the SE keys; shows prompts | `AgentKit` | sandboxed |

### Engine protocol and adapter

Glasshouse **owns the PTY** and treats the engine purely as a parser and renderer. That design is what makes termios and foreground-process inspection possible, and it is also how libghostty-vt works, since it has no PTY of its own. The protocol below is a proposal. The SwiftTerm member names in the adapter are **[UNVERIFIED]** against 1.20.0 and must be checked against the `v1.x` branch.

```swift
// TerminalCore/TerminalEngine.swift — Glasshouse-defined (proposal)
public struct GridSize: Sendable, Equatable { public var cols, rows: UInt16; public var cellPx: CGSize }

public enum SemanticEvent: Sendable {
    case promptStart, inputStart, outputStart                    // OSC 133 A/B/C
    case commandFinished(exitCode: Int32?)                       // OSC 133 D
    case cwd(URL)                                                // OSC 7
    case notify(title: String?, body: String)                    // OSC 9 / 777 / 99 (untrusted)
    case progress(state: Int, value: Int?)                       // OSC 9;4
    case title(String)                                           // OSC 0/2 (untrusted)
    case clipboardWrite(Data), clipboardReadRequest              // OSC 52
}

@MainActor
public protocol TerminalEngine: AnyObject {
    var view: NSView { get }                                     // CAMetalLayer-backed, NSTextInputClient, AX
    var onSendToPTY: ((ArraySlice<UInt8>) -> Void)? { get set }  // keyboard/mouse/paste → bytes
    var onSemanticEvent: ((SemanticEvent) -> Void)? { get set }
    func feed(_ bytes: ArraySlice<UInt8>)                        // PTY output → parser
    func resize(to size: GridSize)
    func scrollToRow(_ absoluteRow: Int)
    func text(inRows: Range<Int>) -> String                      // copy-output, AX
    func applyTheme(_ theme: ResolvedTheme)                      // already APCA-adjusted
}

// TerminalCore/SwiftTermEngine.swift — adapter (SwiftTerm names [UNVERIFIED] for 1.20.0)
import SwiftTerm
@MainActor final class SwiftTermEngine: NSObject, TerminalEngine, TerminalViewDelegate {
    private let tv = TerminalView(frame: .zero)                  // not LocalProcessTerminalView: we own the PTY
    var view: NSView { tv }
    var onSendToPTY: ((ArraySlice<UInt8>) -> Void)?
    var onSemanticEvent: ((SemanticEvent) -> Void)?
    override init() { super.init(); tv.terminalDelegate = self }
    func feed(_ bytes: ArraySlice<UInt8>) { tv.feed(byteArray: bytes) }          // [UNVERIFIED]
    func send(source: TerminalView, data: ArraySlice<UInt8>) { onSendToPTY?(data) } // delegate [UNVERIFIED]
    // sizeChanged / setTerminalTitle / hostCurrentDirectoryUpdate / OSC 133 hook: map to SemanticEvent
    // ... remaining members elided
}
```

### PTY and environment handling

Use `openpty` with an initial `winsize`, then `fork`, then `login_tty(slave)` and `execve` in the C shim. Swift code must not run between `fork` and `exec`. `login_tty` creates a new session, makes the slave the controlling terminal, and duplicates it onto stdin, stdout and stderr ([forkpty(3)](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man3/forkpty.3.html)). Resize with `ioctl(master, TIOCSWINSZ)`, which makes the kernel send SIGWINCH to the foreground process group. Read the PTY on a dedicated `DispatchIO` queue and hop to `@MainActor` only to invalidate the view. Watch child exits carefully; SwiftTerm 1.18 fixed exit events that were being dropped for processes that exit quickly ([SwiftTerm releases](https://github.1485827954.workers.dev/migueldeicaza/SwiftTerm/releases)). Close both file descriptors when a pane closes, a leak alacritty_terminal had to fix ([alacritty CHANGELOG](https://docs.rs/crate/alacritty_terminal/0.24.2-rc1/source/CHANGELOG.md)).

| Variable | Value | Reason |
|---|---|---|
| Shell / argv[0] | `getpwuid(getuid())->pw_shell`, argv[0] = `-zsh` | A leading `-` starts a login shell, matching Terminal.app's behavior |
| `TERM` | `xterm-256color` (v1) | Avoids needing terminfo on remote hosts. Remote programs detect Glasshouse with XTVERSION, because `TERM_PROGRAM` isn't forwarded over ssh ([D'Amore](https://scour.ing/@abnv/p/https://garrett.damore.org/2025/12/macos-terminal-still-missing-mark-apple.html)). A custom `xterm-glasshouse` with Ghostty-style terminfo propagation is deferred to v2 ([Ghostty](https://www.mintlify.com/ghostty-org/ghostty/features/shell-integration)) |
| `COLORTERM` | `truecolor` | |
| `TERM_PROGRAM`, `TERM_PROGRAM_VERSION` | `Glasshouse`, build version | |
| `LANG` | Inherited, or `en_US.UTF-8` if missing | GUI launches often lack a UTF-8 locale |
| `GLASSHOUSE_RESOURCES_DIR`, `GLASSHOUSE_SHELL_FEATURES` | Bundle path; `marks,cwd,title` | Same pattern as Ghostty's `GHOSTTY_SHELL_FEATURES` |
| `SSH_AUTH_SOCK` | **Inherited unchanged** | The agent is reached through `IdentityAgent` per host, never through a global override |
| `SSH_SK_PROVIDER` | Not set by default; the setup assistant offers to add it to `~/.zprofile` | Keeps the user's dotfiles in charge |

### Shell-integration injection without editing dotfiles

This follows Ghostty's mechanism ([Ghostty shell-integration README](https://git.uoc.run.place/Applied-Software/ghostty/raw/commit/9117842a45c311a77b1dbfd99871f43c21f4fb67/src/shell-integration/README.md)). **zsh:** save the user's `ZDOTDIR` (or note that it was unset) into `GLASSHOUSE_ORIG_ZDOTDIR` and point `ZDOTDIR` at `Resources/shell-integration/zsh`. The bundled `.zshenv` puts the original `ZDOTDIR` back, sources the user's real `.zshenv`, and installs `precmd`/`preexec` hooks. Because `ZDOTDIR` is already restored by then, the rest of startup (`.zprofile`, `.zshrc`) reads the user's own files. **bash:** start `bash --posix` with `ENV=<bundle>/bash/glasshouse.bash`. That script runs `set +o posix`, sources the expected login files, and installs `PROMPT_COMMAND`/`PS0` hooks. This requires bash 4.4+, so the stock macOS `/bin/bash` 3.2 only gets marks based on the Return key. **fish:** prepend the bundle path to `XDG_DATA_DIRS` so that `vendor_conf.d` loads it.

```zsh
# Resources/shell-integration/zsh/.zshenv  (sketch)
if [[ -n "${GLASSHOUSE_ORIG_ZDOTDIR+x}" ]]; then ZDOTDIR=$GLASSHOUSE_ORIG_ZDOTDIR; else unset ZDOTDIR; fi
unset GLASSHOUSE_ORIG_ZDOTDIR
[[ -f "${ZDOTDIR:-$HOME}/.zshenv" ]] && source "${ZDOTDIR:-$HOME}/.zshenv"
[[ -o interactive ]] || return
autoload -Uz add-zsh-hook
_gh_precmd()  { local ec=$?; print -n "\e]133;D;${ec}\a\e]7;file://${HOST}${PWD}\a\e]133;A\a"; }
_gh_preexec() { print -n "\e]133;C\a"; }   # command text is display-only/untrusted
add-zsh-hook precmd  _gh_precmd
add-zsh-hook preexec _gh_preexec
PS1="${PS1}%{$(print -n '\e]133;B\a')%}"
```

### Security plumbing sketches

```swift
// SecurityUX/TermiosWatcher.swift
import Darwin
struct PTYSecretState: Equatable, Sendable { var noEcho: Bool; var canonical: Bool
    var isSecretPrompt: Bool { noEcho && canonical } }

func readSecretState(masterFD: Int32) -> PTYSecretState? {
    var t = termios()
    guard tcgetattr(masterFD, &t) == 0 else { return nil }   // master reflects slave line discipline on Darwin [UNVERIFIED]
    return .init(noEcho: t.c_lflag & tcflag_t(ECHO) == 0, canonical: t.c_lflag & tcflag_t(ICANON) != 0)
}

// SecurityUX/SecureInputController.swift — balanced, per-pane (TN2150)
import Carbon.HIToolbox
@MainActor final class SecureInputController {
    private var holders = Set<ObjectIdentifier>()
    func update(pane: AnyObject, wantsSecure: Bool) {
        let id = ObjectIdentifier(pane)
        if wantsSecure, holders.insert(id).inserted, holders.count == 1 { EnableSecureEventInput() }
        if !wantsSecure, holders.remove(id) != nil, holders.isEmpty { DisableSecureEventInput() }
    }
}

// App/AgentRegistration.swift
import ServiceManagement
func enableAgent() throws -> SMAppService.Status {
    let svc = SMAppService.loginItem(identifier: "com.example.glasshouse.agent")
    try svc.register()
    if svc.status == .requiresApproval { SMAppService.openSystemSettingsLoginItems() }
    return svc.status
}

// DesignSystem/GlassCard.swift — L3 transient card (SwiftUI)
struct GlassCard<Content: View>: View {
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency   // [COMMUNITY]
    @ViewBuilder var content: Content
    var body: some View {
        content.padding(16)
            .glassEffect(reduceTransparency ? .identity : .regular,          // never .clear over text
                         in: .rect(cornerRadius: 16))                        // shape spelling [COMMUNITY]
    }
}
```

### Packaging and distribution

```
Glasshouse.app/Contents/
  MacOS/Glasshouse                                   hardened runtime, NOT sandboxed
  Library/LoginItems/GlasshouseAgent.app             LSUIElement, sandboxed, hardened, own provisioning profile
  Resources/shell-integration/{zsh,bash,fish}/       injected scripts
  Resources/Glasshouse.sdef                          AppleScript dictionary
  embedded.provisionprofile                          Developer ID profile (keychain-access-groups)
```

| Item | Main app | Agent | Source / note |
|---|---|---|---|
| App Sandbox | **Off**: children inherit the sandbox, and a sandboxed zsh fails with `can't set tty pgrp` | On | [Dev Forums 685544](https://developer.apple.com/forums/thread/685544) |
| Distribution | Developer ID; not the Mac App Store (App Review requires all code to be sandboxed) | Embedded | [Dev Forums 685544](https://developer.apple.com/forums/thread/685544) |
| Hardened runtime | On; no JIT; no `disable-library-validation` unless plugins arrive | On; no `get-task-allow` | Practice-based **[UNVERIFIED]** list |
| Keychain access group | `$(TeamIdentifierPrefix)com.example.glasshouse` | Same | [TN3137](https://developer.apple.com/documentation/technotes/tn3137-on-mac-keychains.md) |
| Notarization | `xcrun notarytool submit --wait`, then `xcrun stapler staple`; every nested binary signed with a secure timestamp | — | Standard flow; not re-fetched this session **[UNVERIFIED]** |
| Deployment target | macOS 26.0 (the glass APIs require it); test on 27 for the opacity slider | 26.0 | `NSGlassEffectView` is 26.0+ ([Apple](https://developer.apple.com/documentation/appkit/nsglasseffectcontainerview.md)) |

### Register of unverified APIs and open conflicts

| Item | Status | Action |
|---|---|---|
| `glassEffect(_:in:)` | **[VERIFIED-DOC]** ([Apple](https://developer.apple.com/documentation/swiftui/view/glasseffect(_:in:))) | Use as-is |
| `NSGlassEffectContainerView.contentView`, `.spacing` | **[VERIFIED-DOC]** | Use as-is |
| `NSGlassEffectView` style enum Swift spelling, `tintColor` type | **[COMMUNITY]**: Rio uses `setStyle:` 0/1 through the ObjC runtime | Check the SDK header |
| `GlassEffectContainer(spacing:content:)`, `glassEffectUnion`, `glassEffectTransition` | **[COMMUNITY]** ([LiquidGlassReference](https://github.com/conorluddy/LiquidGlassReference)) | Check the SDK |
| `Glass.interactive()` on macOS | WWDC says "On iOS"; the community reference says iOS-only | Don't rely on it |
| `scrollEdgeEffectStyle(_:for:)` | **[VERIFIED-WWDC]** usage, signature unconfirmed | Not applicable to the Metal grid anyway |
| SwiftTerm `TerminalView.feed(byteArray:)`, `send(source:data:)`, OSC 133 delegate hook in 1.20.0 | **[UNVERIFIED]** | Read the `v1.x` branch source |
| `KERN_PROCARGS2` / `proc_pidpath` on setuid sudo | **[UNVERIFIED]** | Prototype first; fall back to the "reported by shell" card |
| `tcgetattr` on the master FD reflecting the slave's `c_lflag` | Used by WezTerm and Ghostty; Darwin specifics **[UNVERIFIED]** | Test with `sudo -k; sudo true` |
| `posix_spawn` + `POSIX_SPAWN_SETSID` as an alternative to fork | **[UNVERIFIED]** | Keep the C fork shim for v1 |
| Clear/Tinted / macOS 27 opacity value readable by apps | No API found | Don't build against it |
| Terminal.app images (terminfo.dev vs ucs-detect) | **[CONFLICT]** | Test by hand |
| Ghostty latency 2 ms vs 13 ms | **[CONFLICT]** | Measure Glasshouse's own latency |
| `agent(plistName:)` vs `loginItem(identifier:)` for a keychain-using agent | **[CONFLICT]**, resolved toward `loginItem` | Revisit if a bare executable can carry a profile |

## Conclusion

The design argument comes down to the fact that **legibility and security have the same requirement in a terminal: keep the content layer free of anything it doesn't control.** An opaque grid is what makes Liquid Glass safe to use, because the glass never has to sit under text, the APCA floor can be checked against a known background, and the Metal layer stays cheap to composite. The same separation makes Glasshouse's trust signals believable. A lock or approval card drawn from kernel state in chrome that output cannot reach is the one prompt-spoofing defense that escape sequences can't defeat. This also changes what "better than Terminal.app" means. Most of the gap is not exotic features. It is parser correctness, making existing features visible (marks, Secure Keyboard Entry, remote connections), and putting context next to Touch ID prompts that currently authorize without saying what for.

The highest-risk unknowns are small and can be tested in about a day each: whether an unprivileged app can read a setuid sudo's argv, whether `tcgetattr` on the PTY master shows the no-echo state on Darwin, and which SwiftTerm 1.20 hooks expose OSC 133. Run those three spikes before writing any UI. If the argv read fails, the sudo card drops to its "reported by shell" trust level, and the optional agent becomes the only path where the Touch ID prompt itself names its destination. For the engine, keep the `TerminalEngine` boundary narrow and the PTY in Glasshouse's own code. That way, when libghostty tags its C API or SwiftTerm 2.0 stabilizes, moving to either one only replaces an adapter.
