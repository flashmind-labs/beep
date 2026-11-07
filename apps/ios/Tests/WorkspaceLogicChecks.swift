import Foundation

@main struct WorkspaceLogicChecks {
    static func main() {
        for mask in 0..<16 {
            let mode = WorkspaceMode(showAll: mask & 1 != 0, hasProject: mask & 2 != 0,
                                     githubConnected: mask & 4 != 0, hasAdditionalMachine: mask & 8 != 0)
            precondition(mode.expanded == (mask != 0), "Each account signal or the override expands navigation")
        }
        print("PASS: 16 workspace mode combinations")
    }
}
