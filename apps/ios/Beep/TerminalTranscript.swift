import Foundation

/// A bounded text transcript for shell commands, including split UTF-8 and ANSI sequences.
struct TerminalTranscript {
    private(set) var text = ""
    private var bytes: [UInt8] = []
    private var escape = ""
    private let limit = 100_000

    mutating func append(_ incoming: [UInt8]) {
        bytes += incoming
        var end = bytes.count
        // Keep an unfinished Unicode scalar for the next chunk.
        while end > max(0, bytes.count - 4), String(bytes: bytes.prefix(end), encoding: .utf8) == nil { end -= 1 }
        guard let chunk = String(bytes: bytes.prefix(end), encoding: .utf8) else { bytes = []; return }
        bytes.removeFirst(end)
        for scalar in chunk.unicodeScalars {
            if !escape.isEmpty {
                escape.unicodeScalars.append(scalar)
                if escape == "\u{1B}[" || escape == "\u{1B}]" { continue }
                if escape.hasPrefix("\u{1B}[") {
                    if scalar.value >= 0x40 && scalar.value <= 0x7E { escape = "" }
                } else if escape.hasPrefix("\u{1B}]") {
                    if scalar.value == 7 || escape.hasSuffix("\u{1B}\\") { escape = "" }
                } else { escape = "" }
                if escape.count > 4096 { escape = "" }
            } else if scalar.value == 0x1B { escape = String(scalar) }
            else if scalar.value == 8 || scalar.value == 127 { if !text.isEmpty { text.removeLast() } }
            else if scalar.value == 13 { continue }
            else if scalar.value == 9 || scalar.value == 10 || scalar.value >= 32 { text.unicodeScalars.append(scalar) }
        }
        if text.count > limit { text = String(text.suffix(limit)) }
    }
}
