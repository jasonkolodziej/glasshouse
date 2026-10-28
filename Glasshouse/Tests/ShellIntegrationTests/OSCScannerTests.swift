import Foundation
import Testing
@testable import ShellIntegration

@Suite("OSC scanning and command timeline")
struct OSCScannerTests {
    private func bytes(_ s: String) -> [UInt8] { Array(s.utf8) }

    @Test func parsesOSC133WithBothTerminators() {
        var s = OSCScanner()
        let events = s.scan(bytes("\u{1B}]133;A\u{07}$ ls\u{1B}]133;C\u{1B}\\out\u{1B}]133;D;2\u{07}"))
        #expect(events == [.promptStart, .outputStart, .commandFinished(exitCode: 2)])
    }

    @Test func handlesSequencesSplitAcrossReads() {
        var s = OSCScanner()
        #expect(s.scan(bytes("text \u{1B}]13")).isEmpty)
        #expect(s.scan(bytes("3;D;0")).isEmpty)
        #expect(s.scan(bytes("\u{07}more")) == [.commandFinished(exitCode: 0)])
    }

    @Test func distinguishesOSC9NotificationFromConEmuProgress() {
        #expect(OSCScanner.parse("9;Build done") == .notification(title: nil, body: "Build done"))
        #expect(OSCScanner.parse("9;4;1;42") == .progress(state: 1, percent: 42))
    }

    @Test func parsesOSC7WorkingDirectory() {
        #expect(OSCScanner.parse("7;file://mac/Users/j/My%20Dir")
                == .workingDirectory(URL(string: "file://mac/Users/j/My%20Dir")!))
    }

    @Test func ignoresNonOSCEscapes() {
        var s = OSCScanner()
        #expect(s.scan(bytes("\u{1B}[31mred\u{1B}[0m")).isEmpty)
    }

    @Test func timelineBuildsRecords() {
        var t = CommandTimeline()
        let t0 = Date(timeIntervalSince1970: 0)
        t.apply(.promptStart, cursorRow: 10, now: t0)
        t.setCommandLine("make test ")
        t.apply(.outputStart, cursorRow: 11, now: t0)
        let done = t.apply(.commandFinished(exitCode: 1), cursorRow: 40, now: t0.addingTimeInterval(7))
        #expect(done?.commandLine == "make test")
        #expect(done?.status == .failed)
        #expect(done?.duration == 7)
        #expect(t.previousPrompt(before: 20)?.promptRow == 10)
    }

    @Test func finishWithoutStartIsIgnored() {
        var t = CommandTimeline()
        t.apply(.promptStart, cursorRow: 0)
        #expect(t.apply(.commandFinished(exitCode: 0), cursorRow: 1) == nil) // empty Return at prompt
    }
}
