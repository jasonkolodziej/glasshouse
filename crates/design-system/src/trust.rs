/// The single trusted-status indicator (critique: collapse four signals into one, three states).
///
/// Rendered as a *filled, tinted* capsule at the toolbar's trailing edge, with white
/// semibold callout text and an icon (never color alone), padded `gh::space::M` × `gh::space::XS`.
/// Program-controlled tab titles are always plain text, so output can't imitate it.
/// Driven only by kernel state. Hidden when [`TrustState::Idle`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum TrustState {
    /// Nothing to report; the capsule is hidden.
    #[default]
    Idle,
    /// termios ECHO off + ICANON on, local pane.
    SecureInput,
    /// Foreground process group contains /usr/bin/sudo.
    TouchIdSudo,
    /// ssh/mosh in the foreground: prompts there are not detectable. Carries the host.
    RemoteSession(String),
}

/// Semantic capsule fill; map to the platform's colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustTint {
    /// Secondary label color.
    Secondary,
    /// System accent color.
    Accent,
    /// System orange.
    Orange,
}

impl TrustState {
    /// SF Symbols name of the capsule icon.
    pub fn symbol(&self) -> &'static str {
        match self {
            Self::Idle => "checkmark.shield",
            Self::SecureInput => "lock.fill",
            Self::TouchIdSudo => "touchid",
            Self::RemoteSession(_) => "globe",
        }
    }

    /// Visible capsule text.
    pub fn label(&self) -> String {
        match self {
            Self::Idle => String::new(),
            Self::SecureInput => "Secure input".to_owned(),
            Self::TouchIdSudo => "Touch ID: sudo".to_owned(),
            Self::RemoteSession(host) => format!("Remote: {host}"),
        }
    }

    /// Accessibility label (the capsule is one element; children are ignored).
    pub fn accessibility_description(&self) -> String {
        match self {
            Self::Idle => "No secure prompt".to_owned(),
            Self::SecureInput => {
                "Secure input on. Keystrokes are hidden from other apps.".to_owned()
            }
            Self::TouchIdSudo => "sudo is waiting for Touch ID in this tab".to_owned(),
            Self::RemoteSession(host) => {
                format!("Remote session to {host}. Password prompts there can't be detected.")
            }
        }
    }

    /// Capsule fill.
    pub fn tint(&self) -> TrustTint {
        match self {
            Self::Idle => TrustTint::Secondary,
            Self::SecureInput | Self::TouchIdSudo => TrustTint::Accent,
            Self::RemoteSession(_) => TrustTint::Orange,
        }
    }

    /// Whether the capsule is shown at all.
    pub fn is_visible(&self) -> bool {
        *self != Self::Idle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_session_text_includes_host() {
        let s = TrustState::RemoteSession("build.local".into());
        assert_eq!(s.label(), "Remote: build.local");
        assert_eq!(
            s.accessibility_description(),
            "Remote session to build.local. Password prompts there can't be detected."
        );
        assert_eq!(s.tint(), TrustTint::Orange);
        assert!(s.is_visible());
        assert!(!TrustState::Idle.is_visible());
    }
}
