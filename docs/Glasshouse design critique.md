# Design Critique: Glasshouse (Liquid Glass terminal)

**Reviewed:** the "Glasshouse keeps the grid opaque…" and "Security design…" sections of *Liquid Glass terminal for macOS* (layer model, layout spec, accessibility fallbacks, flows A–C, approval card).
**Stage:** pre-visual specification. Feedback targets structure, hierarchy and behavior rather than pixels.
**Status:** every recommendation marked ✅ below has been applied in the Swift scaffold.

## Overall Impression

The core decision is right and rare: the grid stays opaque content, and glass lives only on controls. That puts Glasshouse on Apple's side of the Liquid Glass rules, where most "glassy terminal" concepts are not. The biggest risk is in the security chrome. It invents four overlapping trust signals (lock, status slot, card, card trust style). It also places them where the user's eye isn't during a Touch ID prompt. This could turn the headline feature into noise users learn to ignore.

## Usability

| # | Finding | Severity | Recommendation |
|---|---------|----------|----------------|
| U1 | **⌘K opens Hosts (Flow A), but ⌘K is "Clear to Start" in Terminal.app.** It's also clear-screen in iTerm2, Ghostty and VS Code. The spec otherwise promises Terminal.app shortcut parity, so this would be wrong on day one for every migrating user. | 🔴 Critical | ✅ Keep ⌘K = clear scrollback. Hosts move to ⇧⌘O ("Open Host…"), and the palette stays on ⇧⌘P. |
| U2 | **The context card is anchored to the pane's top edge, but `pam_tid`'s Touch ID sheet is a system panel Glasshouse can't position.** The eye goes to the system sheet, and the card sits in peripheral vision exactly when it's needed. | 🔴 Critical | ✅ Anchor the card at the pane's *bottom* edge, beside the cursor, where the user just typed `sudo`. Give it a live "Waiting for Touch ID…" state so it reads as the explanation of the sheet, not a second dialog. |
| U3 | **"No card ⇒ not from this terminal" doesn't hold.** It fails whenever the Glasshouse window is hidden, on another Space, or minimised, which is exactly when a background script runs sudo. Users who internalise the rule will draw wrong conclusions. | 🟡 Moderate | ✅ When the requesting pane isn't visible, post a notification ("Tab 3: sudo systemctl restart nginx is waiting for Touch ID") instead of the card. Teach a narrower rule: "a card or notification always names the command." |
| U4 | **The sticky command header covers grid row 0.** In the normal screen it hides real output, the very row a user may be reading. | 🟡 Moderate | ✅ Show it only when the viewport is scrolled away from the bottom *and* the command's prompt is above the viewport. It then hides a row the user has scrolled past, not live output. Hide it in the alternate screen. |
| U5 | **The overlay gutter "takes no grid columns", so ticks paint over the last column of text.** At 8–10 pt, ticks also make a poor click target (pointer accuracy degrades below about 16 pt). | 🟡 Moderate | ✅ Reserve a 12 pt trailing inset outside the cell grid for the gutter. Draw 6 pt ticks with a 16 pt-tall hit area. Hovering a tick shows a popover with the command, exit code and duration. |
| U6 | **The sidebar is described but its default state isn't.** Terminal users optimise for columns, and a 220–280 pt sidebar open by default costs about 30 columns at 13 pt. | 🟢 Minor | ✅ Collapsed by default. The window remembers its state, and ⌃⌘S toggles it (matching Finder and Xcode). |
| U7 | **Card focus behaviour: "VoiceOver reads the whole card first" implies moving focus.** Stealing focus from the grid mid-command breaks screen-reader flow, and keyboard users then have to return to the pane. | 🟡 Moderate | ✅ Never move focus. Post an `NSAccessibility` announcement with high priority, and expose the card as a labelled element reachable with VO-navigation. |

## Visual Hierarchy

- **What draws the eye first:** the glass toolbar with tabs. That's correct at rest. During a sudo prompt the system sheet should win, and the Glasshouse card should rank second, next to the cursor (U2).
- **Reading flow:** toolbar → grid top → grid bottom/cursor. Every transient that relates to the *current command* (card, sticky header, finish badges) should live near the cursor or the gutter, not the top chrome.
- **Emphasis problem: four trust signals for one event.** A local sudo produces a status-slot label ("sudo · Touch ID"), the card, then possibly the 🔒 lock, and the card's trust style. ✅ Collapse this into **one status capsule with three states**: `Secure input` (lock), `Touch ID: sudo` (fingerprint), `Remote session` (globe; prompts not detectable). The card exists only during an approval, and its trust level is a single "Source: kernel / reported by shell" line rather than a whole alternative visual style.
- **Status slot vs. tab titles.** Tab titles are program-controlled (OSC 0/2) and sit right next to the trusted slot. Stripping lock glyphs isn't enough, because text alone ("🔐 sudo OK") can still mimic it. ✅ Render the trusted slot as a *filled, tinted* capsule at the toolbar's trailing edge. Tab titles are always plain text in glass, never filled. The capsule also gets `visibilityPriority = .high` so it never collapses into the overflow menu on narrow windows.

## Consistency

| Element | Issue | Recommendation |
|---------|-------|----------------|
| Shortcuts | ⌘K conflict (U1). The bookmark shortcut was flagged as conflicting in research. | ✅ Copy Apple's current Terminal.app shortcut table verbatim for every overlapping action, and add new actions only on unused chords. |
| Glass variants | The spec mixes system containers (NSToolbar, sidebar) with hand-placed `NSGlassEffectView`/`.glassEffect` for the palette, card and find bar. Corner radii and padding will drift. | ✅ A single `GlassChrome` modifier owns radius, padding, `.regular` variant and the Reduce Transparency fallback, and every custom glass surface goes through it. |
| Tint | "Only the active tab gets an accent tint", while the status capsule is also tinted. That makes two tinted things in one toolbar. | ✅ The active tab uses a fill plus bold weight, not tint. Tint is reserved for the trust capsule and the card's primary action. |
| Motion | The card "slides out" and the palette "morphs", but no shared motion spec exists. | ✅ Use one spring (`.smooth(duration: 0.25)`) for all L3 transients, and `.identity` under Reduce Motion. |

## Accessibility

- **Color contrast:** default foreground at Lc 75 is sound. **The ANSI floor at Lc 45 is too low for 13 pt body text.** APCA reserves Lc 45 for large or heavy text, and terminal error output (red on dark) is exactly the text users must read. ✅ Raise the ANSI floor to Lc 60, keeping 45 as an opt-out "theme-faithful" setting, and Lc 75 under Increase Contrast. Also apply the floor to the *sticky header* and *gutter popover*. Both are solid surfaces Glasshouse owns, so their contrast is deterministic.
- **Pointer targets:** gutter ticks need 16 pt hit areas (U5). Toolbar items use system sizes, which is fine.
- **Text readability:** "SF Mono 13" is a reasonable default, but no line-height spec exists. ✅ Default cell height = 1.2 × font line height, user-adjustable. Never apply Dynamic Type scaling to the grid; use ⌘+/⌘− like Terminal.app.
- **Color-only meaning:** handled well, since failed ticks differ in shape as well as color. Extend the same rule to the status capsule (icon + word, never color alone).
- **Reduce Transparency:** correctly delegated to system containers. ✅ The scaffold reads `accessibilityReduceTransparency` / `colorSchemeContrast` in SwiftUI and forces opaque fallbacks for custom surfaces.

## What Works Well

- **Opaque grid with glass only on controls.** This is the decision most competitors get wrong, and it's grounded in Apple's own rule.
- **Trust from kernel state, not bytes.** Termios plus foreground-process detection makes WallEscape-style spoofing structurally impossible to render in trusted chrome.
- **Overlays instead of a Warp-style line editor.** Users keep vi-mode, completions and TUI apps unchanged, and everything degrades to Terminal.app behaviour without shell integration.
- **Visible setup scripts instead of hidden privileged calls.** The setup is honest and teachable, and it doubles as documentation.
- **Explicit "what we can't see" state for remote sessions.** Saying "prompts not detectable" builds more trust than pretending.

## Priority Recommendations

1. **Fix the shortcut collision (U1) and adopt Terminal.app's table wholesale.** Migration friction is the first thing users hit, and it costs nothing to fix.
2. **Collapse security signals into one three-state capsule plus a cursor-anchored card (U2, hierarchy).** This is the feature that differentiates Glasshouse, and right now it's spread across four places and positioned away from the user's attention.
3. **Make the overlays non-occluding (U4, U5) and raise the ANSI contrast floor to Lc 60.** An opaque grid only pays off if nothing Glasshouse adds sits on top of text or drops below readable contrast.
