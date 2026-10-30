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
        .library(name: "SSHConfig", targets: ["SSHConfig"]),
    ],
    traits: [
        // Default build targets SwiftTerm 1.99.1 (the 2.0 preview API). Enable this trait only if you
        // switch the dependency below to upstream 1.20.x — it switches the adapter to the old API.
        .trait(name: "SwiftTerm1", description: "Build the engine adapter against SwiftTerm 1.20.x"),
    ],
    dependencies: [
        // SwiftTerm 1.99.1 plus one fix: LocalProcess teardown runs at the IO workers' QoS, so closing
        // a tab no longer makes the parse thread wait on a utility-QoS thread (priority inversion at
        // TerminalIOPipeline.swift:496). Filed upstream as migueldeicaza/SwiftTerm fix/teardown-qos;
        // once a release carries it, go back to
        //   .package(url: "https://github.com/migueldeicaza/SwiftTerm", "1.20.0"..<"2.0.0")
        // Both API generations are supported (see SwiftTermEngine.swift). 1.99.0 is NOT supported
        // (no terminalContentSnapshot); 2.0.0 final is excluded until checked against the adapter.
        .package(url: "https://github.com/jasonkolodziej/SwiftTerm",
                 revision: "783ae5091faeb986132135e8dd71bf1b13712dc8"),
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
        // Managed ~/.ssh/glasshouse.conf for Secure Enclave keys: validated hosts, the Include in ~/.ssh/config,
        // key discovery and setup scripts. Foundation only, so its tests also run on Linux.
        .target(name: "SSHConfig"),

        .testTarget(name: "SSHAgentCoreTests", dependencies: ["SSHAgentCore"]),
        .testTarget(name: "ShellIntegrationTests", dependencies: ["ShellIntegration"]),
        .testTarget(name: "DesignSystemTests", dependencies: ["DesignSystem"]),
        .testTarget(name: "SSHConfigTests", dependencies: ["SSHConfig"], resources: [.copy("Fixtures")]),
    ],
    swiftLanguageModes: [.v6]
)
