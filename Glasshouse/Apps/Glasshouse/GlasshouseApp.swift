import GlasshouseSecurity
import SwiftUI

@main
struct GlasshouseApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @FocusedValue(Workspace.self) private var workspace

    var body: some Scene {
        WindowGroup("Glasshouse") {
            WorkspaceView()
                .frame(minWidth: 520, minHeight: 320)
        }
        .windowToolbarStyle(.unified(showsTitle: false))
        .commands { GlasshouseCommands(workspace: workspace) }

        WindowGroup("Theme Editor", id: ThemeEditorView.windowID, for: String.self) { $themeID in
            ThemeEditorView(themeID: themeID ?? "")
        }
        .defaultSize(width: 980, height: 680)

        Settings { SettingsView() }
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        Notifier.requestAuthorization()
    }
    func applicationWillTerminate(_ notification: Notification) {
        MainActor.assumeIsolated { SecureKeyboardEntry.shared.releaseAll() } // pair every Enable with a Disable
    }
    func applicationDidResignActive(_ notification: Notification) {
        MainActor.assumeIsolated { SecureKeyboardEntry.shared.update(secretPromptFocused: false) }
    }
}

/// Shortcuts copy Terminal.app's table wherever an action overlaps (critique U1 / consistency);
/// new actions take unused chords only.
struct GlasshouseCommands: Commands {
    let workspace: Workspace?

    var body: some Commands {
        CommandGroup(after: .newItem) {
            Button("New Tab") { workspace?.newTab() }.keyboardShortcut("t")
            Button("Open Host…") { workspace?.showsPalette = true }      // was ⌘K in the spec; ⌘K = clear
                .keyboardShortcut("o", modifiers: [.command, .shift])
        }
        // Replaces the default ⌘W Close (Glasshouse saves no documents). Terminal.app table:
        // ⌘W closes the tab (and the window with its last tab), ⇧⌘W closes the window.
        CommandGroup(replacing: .saveItem) {
            Button("Close Tab") {
                if let workspace { workspace.closeSelected() } else { NSApp.keyWindow?.performClose(nil) } // e.g. Settings
            }
            .keyboardShortcut("w")
            Button("Close Window") { NSApp.keyWindow?.performClose(nil) }
                .keyboardShortcut("w", modifiers: [.command, .shift])
        }
        CommandMenu("Shell") {
            Button("Command Palette…") { workspace?.showsPalette = true }
                .keyboardShortcut("p", modifiers: [.command, .shift])
            Divider()
            Button("Jump to Previous Command") { workspace?.selected?.jumpToPreviousPrompt() }
                .keyboardShortcut(.upArrow, modifiers: .command)          // Terminal.app
            Button("Jump to Next Command") { workspace?.selected?.jumpToNextPrompt() }
                .keyboardShortcut(.downArrow, modifiers: .command)        // Terminal.app
            Button("Copy Last Output") { workspace?.selected?.copyLastOutput() }
                .keyboardShortcut("a", modifiers: [.command, .shift])     // Terminal.app: select output
            Divider()
            Button("Clear Scrollback") { workspace?.selected?.clearScrollback() }
                .keyboardShortcut("k")                                    // Terminal.app: Clear to Start
            Button("Secure Keyboard Entry (Always)") {
                SecureKeyboardEntry.shared.alwaysOn.toggle()
            }
        }
        CommandGroup(after: .textEditing) {
            Button("Find…") { workspace?.showsFind = true }.keyboardShortcut("f")
        }
        CommandGroup(after: .sidebar) {
            Button("Toggle Sessions Sidebar") { workspace?.showsSidebar.toggle() }
                .keyboardShortcut("s", modifiers: [.command, .control])
        }
    }
}
