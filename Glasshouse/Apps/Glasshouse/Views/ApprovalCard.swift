import DesignSystem
import SwiftUI

/// The Touch ID context card. Apple's pam_tid sheet doesn't say *what* you're approving;
/// this card does, from kernel state, in chrome that program output can't draw into.
///
/// It does not steal focus (critique U7): VoiceOver gets a high-priority announcement from the
/// session, and the card is reachable with VO navigation.
struct ApprovalCard: View {
    let context: ApprovalContext

    var body: some View {
        VStack(alignment: .leading, spacing: GH.Space.s) {
            // L4 header: fixed wording and symbol, never derived from program output.
            HStack(spacing: GH.Space.s) {
                Image(systemName: context.phase == .waitingForTouchID ? "touchid" : "lock.fill")
                    .font(.title2)
                    .symbolEffect(.pulse, isActive: context.phase == .waitingForTouchID)
                Text(context.phase == .waitingForTouchID ? "Waiting for Touch ID…" : "Password requested")
                    .font(.headline)
                Spacer()
            }

            Grid(alignment: .leading, horizontalSpacing: GH.Space.m, verticalSpacing: GH.Space.xs) {
                row("Command", context.command, mono: true)
                row("Run as", context.runAs)
                row("Host", context.host)
                row("From", [context.tabTitle, context.directory].compactMap { $0 }.joined(separator: " · "))
            }

            // One line, not a separate visual style (critique: collapse trust signals).
            Label("Source: \(context.source.rawValue)",
                  systemImage: context.source == .kernel ? "checkmark.seal" : "exclamationmark.triangle")
                .font(.caption)
                .foregroundStyle(context.source == .kernel ? Color.secondary : Color.orange)

            if context.phase == .passwordFallback {
                Text("Touch ID was cancelled or unavailable. Secure input is on for this tab.")
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
        .padding(GH.Space.l)
        .frame(width: GH.Size.cardWidth)
        .glassChrome(.transient, cornerRadius: GH.Radius.card, tint: nil)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(
            "Approval request. sudo \(context.command), run as \(context.runAs), on \(context.host). Source: \(context.source.rawValue).")
    }

    @ViewBuilder private func row(_ key: String, _ value: String, mono: Bool = false) -> some View {
        GridRow {
            Text(key).foregroundStyle(.secondary).gridColumnAlignment(.trailing)
            Text(value)
                .font(mono ? .system(.body, design: .monospaced).weight(.semibold) : .body)
                .lineLimit(3)
                .textSelection(.enabled)
        }
    }
}
