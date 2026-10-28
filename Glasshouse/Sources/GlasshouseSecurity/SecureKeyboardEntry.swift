import Carbon.HIToolbox

/// Secure Keyboard Entry, scoped the way Apple's guidance recommends: on only while a secret prompt
/// has focus, because it is system-wide (it blocks other apps' event taps, including some accessibility
/// tools). Calls to Enable/DisableSecureEventInput are reference-counted by the system, so every
/// enable must be paired with exactly one disable — this type guarantees that.
@MainActor
public final class SecureKeyboardEntry {
    public static let shared = SecureKeyboardEntry()
    public private(set) var isEnabled = false

    /// User preference: "Always on" mirrors Terminal.app's menu item (CIS benchmark setting).
    public var alwaysOn = false {
        didSet { update(secretPromptFocused: lastFocused) }
    }
    private var lastFocused = false

    public func update(secretPromptFocused: Bool) {
        lastFocused = secretPromptFocused
        let want = alwaysOn || secretPromptFocused
        guard want != isEnabled else { return }
        if want { EnableSecureEventInput() } else { DisableSecureEventInput() }
        isEnabled = want
    }

    /// Call from applicationWillTerminate / when the window resigns key.
    public func releaseAll() { update(secretPromptFocused: false); if alwaysOn { alwaysOn = false } }

    /// True if *any* process currently holds secure input (shown in the status bar as a lock).
    public static var isActiveSystemWide: Bool { IsSecureEventInputEnabled() }
}
