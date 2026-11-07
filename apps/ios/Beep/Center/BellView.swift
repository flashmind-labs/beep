import SwiftUI

/// The last notices from the asks feed, with the options of a question to answer it here.
struct BellView: View {
    let center: AskCenter
    @Binding var path: [Route]
    @State private var error = ""

    private var today: [MobileNotice] { center.log.notices.filter { Calendar.current.isDateInToday(date($0)) } }
    private var earlier: [MobileNotice] { center.log.notices.filter { !Calendar.current.isDateInToday(date($0)) } }

    var body: some View {
        VStack(spacing: 0) {
            CCTop(title: "Notifications", back: { if !path.isEmpty { path.removeLast() } }) {
                if !center.log.notices.isEmpty {
                    Button("Clear") { center.clear() }.buttonStyle(TextAction())
                }
            }
            CCBody {
                WorkspaceError(text: error)
                if center.log.notices.isEmpty {
                    CCPanel { CCEmpty(title: "No notifications", detail: "Questions and finished work show here.") }
                } else {
                    if !today.isEmpty { CCSection("Today") { CCPanel { ForEach(today) { row($0) } } } }
                    if !earlier.isEmpty { CCSection("Earlier") { CCPanel { ForEach(earlier) { row($0) } } } }
                }
            }
        }
        .background(Mint.page).foregroundStyle(Mint.ink)
        .toolbar(.hidden, for: .navigationBar)
    }

    private func date(_ notice: MobileNotice) -> Date { Date(timeIntervalSince1970: Double(notice.atMs) / 1000) }

    @ViewBuilder private func row(_ notice: MobileNotice) -> some View {
        let time = Text(date(notice), style: .time)
        switch notice.kind {
        case .ask:
            VStack(spacing: 0) {
                CCRow(title: notice.title, subtitle: notice.detail, action: { open(notice) }) {
                    CCAgentTile(agent: notice.agent)
                } right: { time }
                options(notice)
            }
        case .finished:
            CCRow(title: notice.title, subtitle: notice.detail, action: { open(notice) }) {
                CCKindTile(text: "\u{2713}", kind: .ok)
            } right: { time }
        case .run:
            let failed = notice.title.contains("failed")
            CCRow(title: notice.title, subtitle: notice.detail) {
                CCKindTile(text: failed ? "\u{2715}" : "\u{2713}", kind: failed ? .bad : .ok)
            } right: { time }
        }
    }

    @ViewBuilder private func options(_ notice: MobileNotice) -> some View {
        let primary = notice.options.first { $0.allow }?.id
        let ordered = notice.options.filter { $0.id != primary } + notice.options.filter { $0.id == primary }
        let short = notice.options.count <= 2 && notice.options.allSatisfy { $0.name.count <= 12 }
        Group {
            if short {
                HStack(spacing: 6) { Spacer(); buttons(notice, ordered, primary, leading: false) }
            } else {
                VStack(spacing: 6) { buttons(notice, notice.options, primary, leading: true) }
            }
        }.padding(.horizontal, 11).padding(.bottom, 10)
    }

    private func buttons(_ notice: MobileNotice, _ options: [AskOption], _ primary: String?, leading: Bool) -> some View {
        ForEach(options, id: \.id) { option in
            CCButton(title: option.name, style: option.id == primary ? .primary : .plain, leading: leading) {
                Task { error = await center.answer(notice, option: option) ?? "" }
            }
        }
    }

    private func open(_ notice: MobileNotice) {
        guard !notice.machineId.isEmpty else { return }
        path.append(.agentSession(notice.machineId, notice.agent, notice.session))
    }
}
