// swift-tools-version: 6.2
// GlasshouseKit — the non-UI core of Glasshouse, a native macOS terminal.
// The app and the SSH-agent login item are Xcode targets (see project.yml) that link these libraries.

import PackageDescription

let package = Package(
    name: "GlasshouseKit",
    platforms: [.macOS(.v26)],
    products: [
        .library(name: "TerminalCore", targets: ["TerminalCore"]),
        .library(name: "ShellIntegration", targets: ["ShellIntegration"]),
        .library(name: "GlasshouseSecurity", targets: ["GlasshouseSecurity"]),
        .library(name: "SSHAgentCore", targets: ["SSHAgentCore"]),
        .library(name: "DesignSystem", targets: ["DesignSystem"]),
    ],
    traits: [
        // Default build targets SwiftTerm 1.99.1+ (the 2.0 preview API). Enable this trait only if you
        // pin SwiftTerm to 1.20.x (e.g. in Package.resolved) — it switches the adapter to the old API.
        .trait(name: "SwiftTerm1", description: "Build the engine adapter against SwiftTerm 1.20.x"),
    ],
    dependencies: [
        // Both API generations are supported (see SwiftTermEngine.swift). SwiftPM resolves the newest
        // in range → 1.99.1+ unless you pin. 1.99.0 is NOT supported (no terminalContentSnapshot);
        // 2.0.0 final is excluded until it has been checked against the adapter.
        .package(url: "https://github.com/migueldeicaza/SwiftTerm", "1.20.0"..<"2.0.0"),
    ],
    targets: [
        // Engine abstraction + SwiftTerm adapter, PTY environment, password-prompt detection.
        .target(
            name: "TerminalCore",
            dependencies: [
                "ShellIntegration",
                .product(name: "SwiftTerm", package: "SwiftTerm"),
            ],
            swiftSettings: [.define("SWIFTTERM_1", .when(traits: ["SwiftTerm1"]))]
        ),
        // OSC 133 / OSC 7 parsing, command marks, shell script injection (zsh ZDOTDIR, fish XDG_DATA_DIRS).
        .target(
            name: "ShellIntegration",
            resources: [.copy("Resources/zsh"), .copy("Resources/fish"), .copy("Resources/bash")]
        ),
        // Touch ID sudo status (pam_tid / sudo_local), native Secure Enclave SSH (sc_auth + ssh-keychain.dylib),
        // Secure Keyboard Entry, foreground-process inspection for trusted-chrome approval cards.
        .target(name: "GlasshouseSecurity"),
        // RFC 9987 ssh-agent wire protocol, Secure Enclave key store, Unix-socket server with peer identification.
        .target(name: "SSHAgentCore", dependencies: ["GlasshouseSecurity"]),
        // Liquid Glass chrome helpers with Reduce Transparency / Increase Contrast fallbacks, APCA contrast.
        .target(name: "DesignSystem"),

        .testTarget(name: "SSHAgentCoreTests", dependencies: ["SSHAgentCore"]),
        .testTarget(name: "ShellIntegrationTests", dependencies: ["ShellIntegration"]),
        .testTarget(name: "DesignSystemTests", dependencies: ["DesignSystem"]),
    ],
    swiftLanguageModes: [.v6]
)
