import SwiftUI

/// The single trusted-status indicator (critique: collapse four signals into one, three states).
/// Rendered as a *filled, tinted* capsule at the toolbar's trailing edge; program-controlled tab
/// titles are always plain text, so output can't imitate it. Driven only by kernel state.
public enum TrustState: Equatable, Sendable {
    case idle
    case secureInput            // termios ECHO off + ICANON on, local pane
    case touchIDSudo            // foreground process group contains /usr/bin/sudo
    case remoteSession(String)  // ssh/mosh in the foreground: prompts there are not detectable

    var symbol: String {
        switch self {
        case .idle: "checkmark.shield"
        case .secureInput: "lock.fill"
        case .touchIDSudo: "touchid"
        case .remoteSession: "globe"
        }
    }

    var label: String {
        switch self {
        case .idle: ""
        case .secureInput: "Secure input"
        case .touchIDSudo: "Touch ID: sudo"
        case .remoteSession(let host): "Remote: \(host)"
        }
    }

    var accessibilityDescription: String {
        switch self {
        case .idle: "No secure prompt"
        case .secureInput: "Secure input on. Keystrokes are hidden from other apps."
        case .touchIDSudo: "sudo is waiting for Touch ID in this tab"
        case .remoteSession(let host): "Remote session to \(host). Password prompts there can't be detected."
        }
    }

    var tint: Color {
        switch self {
        case .idle: .secondary
        case .secureInput, .touchIDSudo: .accentColor
        case .remoteSession: .orange
        }
    }
}

public struct TrustCapsule: View {
    public let state: TrustState
    public init(state: TrustState) { self.state = state }

    public var body: some View {
        Group {
            if state == .idle {
                EmptyView()
            } else {
                Label(state.label, systemImage: state.symbol)   // icon + word: never color alone
                    .font(.callout.weight(.semibold))
                    .padding(.horizontal, GH.Space.m)
                    .padding(.vertical, GH.Space.xs)
                    .foregroundStyle(.white)
                    .background(Capsule().fill(state.tint))
                    .accessibilityElement(children: .ignore)
                    .accessibilityLabel(state.accessibilityDescription)
                    .ghTransition(from: .top)
            }
        }
        .animation(GH.motion, value: state)
    }
}
