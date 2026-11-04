struct WorkspaceMode {
    var showAll = false
    var hasProject = false
    var githubConnected = false
    var hasAdditionalMachine = false

    var expanded: Bool { showAll || hasProject || githubConnected || hasAdditionalMachine }
}
