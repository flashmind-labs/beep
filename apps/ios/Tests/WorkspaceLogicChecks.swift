import Foundation

@main struct WorkspaceLogicChecks {
    static func main() {
        for mask in 0..<16 {
            let mode = WorkspaceMode(showAll: mask & 1 != 0, hasProject: mask & 2 != 0,
                                     githubConnected: mask & 4 != 0, hasAdditionalMachine: mask & 8 != 0)
            precondition(mode.expanded == (mask != 0), "Each account signal or the override expands navigation")
        }
        var text = TerminalTranscript()
        text.append(Array("$ echo café\r\n".utf8).dropLast().map { $0 })
        text.append([10])
        precondition(text.text == "$ echo café\n", "CRLF is one newline")
        var split = TerminalTranscript()
        split.append([0x63, 0x61, 0x66, 0xC3])
        split.append([0xA9])
        precondition(split.text == "café", "UTF-8 may cross output chunks")
        split.append(Array("\u{1B}[3".utf8))
        split.append(Array("2m done\u{1B}[0m".utf8))
        precondition(split.text == "café done", "ANSI sequences may cross output chunks")
        split.append(Array("\u{1B}]0;Shell\u{07}ab\u{08}c".utf8))
        precondition(split.text == "café doneac", "Title escapes are hidden and backspace removes one character")
        var bounded = TerminalTranscript()
        bounded.append(Array(String(repeating: "x", count: 110_000).utf8))
        precondition(bounded.text.count == 100_000, "Output stays bounded")
        print("PASS: 16 workspace mode combinations; CRLF, split UTF-8, split ANSI, title escapes, backspace, bounded output")
    }
}
