import Foundation

/// SSH wire encoding (RFC 4251 §5): uint32 big-endian, length-prefixed strings, two's-complement mpints.
public enum SSHWireError: Error, Equatable { case truncated, tooLarge, malformed(String) }

public struct SSHWriter: Sendable {
    public private(set) var data = Data()
    public init() {}

    public mutating func byte(_ b: UInt8) { data.append(b) }
    public mutating func bool(_ b: Bool) { data.append(b ? 1 : 0) }
    public mutating func uint32(_ v: UInt32) {
        withUnsafeBytes(of: v.bigEndian) { data.append(contentsOf: $0) }
    }
    public mutating func string(_ bytes: Data) { uint32(UInt32(bytes.count)); data.append(bytes) }
    public mutating func string(_ s: String) { string(Data(s.utf8)) }

    /// Unsigned magnitude → mpint: strip leading zeros; prepend 0x00 if the top bit is set (keeps it positive).
    public mutating func mpint(unsigned magnitude: Data) { string(Self.mpintBytes(magnitude)) }

    public static func mpintBytes(_ magnitude: Data) -> Data {
        var bytes = magnitude.drop { $0 == 0 }
        if let first = bytes.first, first & 0x80 != 0 { bytes.insert(0, at: bytes.startIndex) }
        return Data(bytes)
    }
}

public struct SSHReader: Sendable {
    private let data: Data
    private var offset: Int
    /// Defensive cap on any single string inside a message.
    public static let maxString = 256 * 1024

    public init(_ data: Data) { self.data = Data(data); self.offset = 0 }

    public var isAtEnd: Bool { offset >= data.count }
    public var remaining: Int { data.count - offset }

    public mutating func byte() throws -> UInt8 {
        guard remaining >= 1 else { throw SSHWireError.truncated }
        defer { offset += 1 }
        return data[data.startIndex + offset]
    }
    public mutating func bool() throws -> Bool { try byte() != 0 }
    public mutating func uint32() throws -> UInt32 {
        guard remaining >= 4 else { throw SSHWireError.truncated }
        let start = data.startIndex + offset
        let v = data[start..<start + 4].reduce(UInt32(0)) { ($0 << 8) | UInt32($1) }
        offset += 4
        return v
    }
    public mutating func string() throws -> Data {
        let n = Int(try uint32())
        guard n <= Self.maxString else { throw SSHWireError.tooLarge }
        guard remaining >= n else { throw SSHWireError.truncated }
        let start = data.startIndex + offset
        offset += n
        return data[start..<start + n]
    }
    public mutating func utf8String() throws -> String {
        guard let s = String(data: try string(), encoding: .utf8) else { throw SSHWireError.malformed("utf8") }
        return s
    }
}
