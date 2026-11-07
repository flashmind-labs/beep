import SwiftUI

enum CenterTab: CaseIterable {
    case home, projects, agents, terminals, machines

    var title: String {
        switch self {
        case .home: "Home"; case .projects: "Projects"; case .agents: "Agents"; case .terminals: "Terminals"; case .machines: "Machines"
        }
    }
    var icon: String {
        switch self {
        case .home: "house"; case .projects: "folder"; case .agents: "bubble.left"; case .terminals: "terminal"; case .machines: "server.rack"
        }
    }
}

/// What a linked phone opens on: the five tabs of the command center.
struct CenterRoot: View {
    @Environment(AppModel.self) private var model
    @Environment(\.scenePhase) private var scenePhase
    @Binding var path: [Route]
    @State private var tab = CenterTab.home
    @State private var home: HomeModel?

    var body: some View {
        VStack(spacing: 0) {
            Group {
                switch tab {
                case .home: if let home { HomeView(home: home, path: $path, openMachines: { tab = .machines }) }
                case .projects: ProjectsView(path: $path)
                case .agents: CodingAgentsView(path: $path)
                case .terminals: TerminalsView(path: $path)
                case .machines: MachinesView(back: {})
                }
            }
            .environment(\.tabRoot, true)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            tabBar
        }
        .background(Mint.page)
        .toolbar(.hidden, for: .navigationBar)
        .task {
            let store = model.workspace
            let made = HomeModel(load: { try await store.client().home() },
                                 send: { try await store.client().answerCoding(machineId: $0, agent: $1, session: $2, question: $3, reply: $4) })
            home = made
            await made.refresh()
        }
        .task(id: scenePhase) {
            if scenePhase == .active { AskCenter.shared.start(client: model.workspace.client) }
            if scenePhase == .background { BackgroundAsks.schedule() }
        }
    }

    private var tabBar: some View {
        HStack(spacing: 0) {
            ForEach(CenterTab.allCases, id: \.self) { item in
                let on = item == tab
                Button { tab = item } label: {
                    VStack(spacing: 2) {
                        Image(systemName: item.icon).font(.system(size: 20)).foregroundStyle(on ? Mint.accent : Mint.muted)
                            .frame(height: 24)
                        Text(item.title).font(.body(10.5)).foregroundStyle(on ? Mint.ink : Mint.muted)
                    }
                    .frame(maxWidth: .infinity).padding(.top, 7).padding(.bottom, 6)
                    .overlay(alignment: .top) {
                        if item == .home, tab != .home, (home?.badge ?? 0) > 0 {
                            Circle().fill(Mint.warn).frame(width: 7, height: 7).offset(x: 12, y: 5)
                        }
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain).accessibilityAddTraits(on ? .isSelected : [])
            }
        }
        .background(Mint.surface, ignoresSafeAreaEdges: .bottom)
        .overlay(alignment: .top) { Rectangle().fill(Mint.soft).frame(height: 1) }
    }
}
