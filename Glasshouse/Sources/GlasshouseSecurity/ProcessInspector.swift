import Darwin
import Foundation

/// What is actually running in the foreground of a PTY. Read from the kernel (tcgetpgrp + sysctl),
/// never from terminal output, so the approval card can show the *real* command, user and host
/// in trusted window chrome — something no shipping terminal does and the system Touch ID sheet omits.
public struct ProcessSnapshot: Equatable, Sendable {
    public let pid: pid_t
    public let executablePath: String
    public let arguments: [String]
    public let effectiveUser: String

    public var executableName: String { URL(fileURLWithPath: executablePath).lastPathComponent }
    public var commandLine: String { arguments.map(Self.shellQuote).joined(separator: " ") }

    public var isSudo: Bool { executableName == "sudo" }
    public var isSSH: Bool { executableName == "ssh" }

    /// For `ssh [opts] [user@]host [cmd]`: the destination argument. Best-effort (no full getopt parse).
    public var sshDestination: String? {
        guard isSSH else { return nil }
        let optsWithValue: Set<Character> = ["b","c","D","E","e","F","I","i","J","L","l","m","O","o","p","Q","R","S","W","w"]
        var i = 1
        while i < arguments.count {
            let a = arguments[i]
            if a == "--" { return i + 1 < arguments.count ? arguments[i + 1] : nil }
            if a.hasPrefix("-"), a.count >= 2 {
                let flag = a[a.index(after: a.startIndex)]
                if optsWithValue.contains(flag) && a.count == 2 { i += 2 } else { i += 1 }
                continue
            }
            return a
        }
        return nil
    }

    static func shellQuote(_ s: String) -> String {
        if s.isEmpty { return "''" }
        let safe = CharacterSet.alphanumerics.union(CharacterSet(charactersIn: "@%+=:,./-_"))
        if s.unicodeScalars.allSatisfy(safe.contains) { return s }
        return "'" + s.replacingOccurrences(of: "'", with: "'\\''") + "'"
    }
}

public enum ProcessInspector {
    /// The foreground process group leader of the terminal attached to `masterFD`.
    public static func foreground(masterFD: Int32) -> ProcessSnapshot? {
        let pgid = tcgetpgrp(masterFD)
        guard pgid > 0 else { return nil }
        return snapshot(pid: pgid)
    }

    public static func snapshot(pid: pid_t) -> ProcessSnapshot? {
        guard let path = executablePath(pid: pid) else { return nil }
        let args = arguments(pid: pid) ?? [path]
        let user = effectiveUID(pid: pid).flatMap(userName) ?? "?"
        return ProcessSnapshot(pid: pid, executablePath: path, arguments: args, effectiveUser: user)
    }

    static func executablePath(pid: pid_t) -> String? {
        var buf = [CChar](repeating: 0, count: 4096) // PROC_PIDPATHINFO_MAXSIZE
        let n = proc_pidpath(pid, &buf, UInt32(buf.count))
        return n > 0 ? String(cString: buf) : nil
    }

    /// argv via sysctl KERN_PROCARGS2: [argc:int32][exec_path\0][padding \0…][argv0\0argv1\0…][env…]
    static func arguments(pid: pid_t) -> [String]? {
        var mib: [Int32] = [CTL_KERN, KERN_PROCARGS2, pid]
        var size = 0
        guard sysctl(&mib, 3, nil, &size, nil, 0) == 0, size > MemoryLayout<Int32>.size else { return nil }
        var data = [UInt8](repeating: 0, count: size)
        guard sysctl(&mib, 3, &data, &size, nil, 0) == 0 else { return nil }

        let argc = data.withUnsafeBytes { $0.load(as: Int32.self) }
        var i = MemoryLayout<Int32>.size
        while i < size, data[i] != 0 { i += 1 }    // skip exec path
        while i < size, data[i] == 0 { i += 1 }    // skip padding
        var args: [String] = []
        var start = i
        while i < size, args.count < Int(argc) {
            if data[i] == 0 {
                args.append(String(decoding: data[start..<i], as: UTF8.self))
                start = i + 1
            }
            i += 1
        }
        return args
    }

    static func effectiveUID(pid: pid_t) -> uid_t? {
        var info = kinfo_proc()
        var size = MemoryLayout<kinfo_proc>.stride
        var mib: [Int32] = [CTL_KERN, KERN_PROC, KERN_PROC_PID, pid]
        guard sysctl(&mib, 4, &info, &size, nil, 0) == 0, size > 0 else { return nil }
        return info.kp_eproc.e_ucred.cr_uid
    }

    static func userName(_ uid: uid_t) -> String? {
        guard let pw = getpwuid(uid) else { return nil }
        return String(cString: pw.pointee.pw_name)
    }
}
