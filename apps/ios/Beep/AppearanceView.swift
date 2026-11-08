import SwiftUI

/// Match system, then the desktop's skins, each with a swatch.
struct AppearanceView: View {
    let back: () -> Void

    private struct Cell: Identifiable {
        let id, name: String
        let page, first, second: Color
    }

    private var cells: [Cell] {
        let linear = Skins.resolve("linear", dark: true).palette, paper = Skins.resolve("paper", dark: false).palette
        return [Cell(id: Skins.system, name: "Match system", page: linear.page, first: linear.accent, second: paper.page)]
            + Skins.all.map { Cell(id: $0.id, name: $0.name, page: $0.palette.page, first: $0.palette.accent, second: $0.palette.ok) }
    }

    var body: some View {
        VStack(spacing: 0) {
            CCTop(title: "Appearance", back: back)
            ScrollView {
                LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 8), count: 3), spacing: 8) {
                    ForEach(cells) { cell in
                        let selected = Appearance.shared.choice == cell.id
                        Button { Appearance.shared.choice = cell.id } label: {
                            VStack(alignment: .leading, spacing: 6) {
                                HStack(alignment: .bottom, spacing: 3) {
                                    RoundedRectangle(cornerRadius: 3).fill(cell.first).frame(width: 10, height: 10)
                                    RoundedRectangle(cornerRadius: 3).fill(cell.second).frame(width: 10, height: 10)
                                    Spacer(minLength: 0)
                                }
                                .padding(5).frame(maxWidth: .infinity, minHeight: 34, alignment: .bottomLeading)
                                .background(RoundedRectangle(cornerRadius: Mint.smallRadius).fill(cell.page))
                                .overlay(RoundedRectangle(cornerRadius: Mint.smallRadius).stroke(.gray.opacity(0.25), lineWidth: 1))
                                Text(cell.name).font(.body(12)).foregroundStyle(Mint.ink).lineLimit(1)
                            }
                            .padding(8)
                            .overlay(RoundedRectangle(cornerRadius: Mint.radius)
                                .stroke(selected ? Mint.accent : Mint.soft, lineWidth: 1))
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(.horizontal, 12).padding(.top, 12)
                CCNote(text: "Match system uses Linear in dark mode and Light in light mode.")
                    .padding(.horizontal, 14).padding(.top, 12).padding(.bottom, 20)
            }
        }
        .background(Mint.page)
    }
}
