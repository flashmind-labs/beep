import SwiftUI

/// Shares a chat as it is now and shows its link, as on Android. Opening it again on the
/// same chat updates the shared copy and keeps the link.
struct ShareChatSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let session: Session
    @State private var link: SharedLink?
    @State private var error = ""
    @State private var copied = false

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 12) {
                Image(systemName: "link").font(.system(size: 22, weight: .semibold)).foregroundStyle(Mint.ink)
                    .frame(width: 52, height: 52)
                    .background(RoundedRectangle(cornerRadius: Mint.radius).fill(Mint.tints[1]))
                VStack(alignment: .leading, spacing: 1) {
                    Text("Share this chat").font(.display(18)).foregroundStyle(Mint.ink)
                    Text(session.title.isEmpty ? "New conversation" : session.title)
                        .font(.body(13)).foregroundStyle(Mint.muted).lineLimit(1)
                }
            }
            SwiftUI.Group {
                if let url = link?.url {
                    Text(url).font(.system(size: 12, design: .monospaced)).foregroundStyle(Mint.ink).lineLimit(1)
                        .truncationMode(.middle)
                } else if error.isEmpty {
                    ProgressView().tint(Mint.muted)
                } else {
                    Text(error).font(.body(12)).foregroundStyle(Mint.error).lineLimit(2)
                }
            }
            .frame(maxWidth: .infinity, minHeight: 46, alignment: .leading)
            .padding(.horizontal, 14)
            .background(RoundedRectangle(cornerRadius: Mint.smallRadius).fill(Mint.page))
            HStack(spacing: 10) {
                Button {
                    UIPasteboard.general.string = link?.url
                    copied = true
                } label: {
                    Label(copied ? "Copied" : "Copy", systemImage: copied ? "checkmark" : "doc.on.doc")
                        .font(.body(14, .semibold)).foregroundStyle(Mint.ink)
                        .frame(maxWidth: .infinity, minHeight: 48)
                        .overlay(RoundedRectangle(cornerRadius: Mint.smallRadius).stroke(Mint.soft, lineWidth: 1))
                }
                .buttonStyle(.plain)
                if let url = link.flatMap({ URL(string: $0.url) }) {
                    ShareLink(item: url, message: Text(session.title)) {
                        Label("Send", systemImage: "square.and.arrow.up")
                            .font(.body(14, .semibold)).foregroundStyle(Mint.ink)
                            .frame(maxWidth: .infinity, minHeight: 48)
                            .background(RoundedRectangle(cornerRadius: Mint.smallRadius).fill(Mint.accent))
                    }
                    .buttonStyle(.plain)
                } else {
                    Label("Send", systemImage: "square.and.arrow.up")
                        .font(.body(14, .semibold)).foregroundStyle(Mint.ink.opacity(0.4))
                        .frame(maxWidth: .infinity, minHeight: 48)
                        .background(RoundedRectangle(cornerRadius: Mint.smallRadius).fill(Mint.soft))
                }
            }
            .disabled(link == nil)
            Text("A copy of the chat as it is now. Later messages are not in it, and sharing again updates it. "
                 + "Only your messages and Beep's replies are shared, not tool results, thoughts or your location. "
                 + "Anyone with Beep can open the link.")
                .font(.body(12)).foregroundStyle(Mint.muted).lineSpacing(3)
                .fixedSize(horizontal: false, vertical: true)
            HStack {
                Button("Stop sharing") {
                    Task {
                        do {
                            try await model.unshare(session.id)
                            dismiss()
                        } catch {
                            self.error = describe(error)
                        }
                    }
                }
                .buttonStyle(TextAction(color: Mint.error))
                .disabled(link == nil)
                Spacer()
                Button("Done") { dismiss() }.buttonStyle(TextAction())
            }
        }
        .padding(.horizontal, Mint.pageInset).padding(.top, 24).padding(.bottom, 12)
        .frame(maxHeight: .infinity, alignment: .top)
        .background(Mint.surface)
        .presentationDetents([.height(440)])
        .presentationDragIndicator(.visible)
        .task(id: session.id) {
            do { link = try await model.share(session.id) } catch { self.error = describe(error) }
        }
    }
}

/// The shared copy behind a share link, read only, with a button to carry it on as one's own chat.
struct SharedChatView: View {
    @Environment(AppModel.self) private var model
    let token: String
    let back: () -> Void
    let continued: () -> Void
    @State private var shared: SharedSession?
    @State private var gone = false
    @State private var error = ""
    @State private var copying = false

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 4) {
                BackArrow(action: back)
                VStack(alignment: .leading, spacing: 1) {
                    Text(shared?.title ?? "Shared chat").font(.display(18)).foregroundStyle(Mint.ink).lineLimit(1)
                    if let shared {
                        Text("Shared " + Date(timeIntervalSince1970: Double(shared.sharedAt) / 1000)
                            .formatted(.relative(presentation: .named)))
                            .font(.body(12)).foregroundStyle(Mint.muted)
                    }
                }
                Spacer()
            }
            .padding(.leading, 6).padding(.trailing, 18).frame(minHeight: 56)
            if gone {
                Text("Not found.\nThe owner stopped sharing this chat, or deleted it.")
                    .font(.body(14)).foregroundStyle(Mint.muted).multilineTextAlignment(.center).lineSpacing(4)
                    .padding(.horizontal, Mint.pageInset).padding(.vertical, 60)
                Spacer()
            } else if let shared {
                ScrollView {
                    LazyVStack(spacing: 12) {
                        ForEach(Array(messages(shared).enumerated()), id: \.offset) { _, message in
                            MessageView(message: message, live: false)
                        }
                    }
                    .padding(.horizontal, Mint.pageInset).padding(.vertical, 8)
                }
                VStack(spacing: 8) {
                    if !error.isEmpty { Text(error).font(.body(12)).foregroundStyle(Mint.error) }
                    Button {
                        copying = true
                        Task {
                            do {
                                try await model.continueShared(token)
                                continued()
                            } catch {
                                copying = false
                                self.error = describe(error)
                            }
                        }
                    } label: {
                        Label("Continue in my chats", systemImage: "plus")
                    }
                    .buttonStyle(PrimaryButton())
                    .disabled(copying)
                    Text("Read only. Continuing makes your own copy.").font(.body(12)).foregroundStyle(Mint.muted)
                }
                .padding(.horizontal, Mint.pageInset).padding(.top, 10).padding(.bottom, 14)
            } else {
                Spacer()
                if error.isEmpty { ProgressView().tint(Mint.muted) } else {
                    Text(error).font(.body(14)).foregroundStyle(Mint.error).padding(20)
                }
                Spacer()
            }
        }
        .background(Mint.page)
        .task(id: token) {
            while !model.connected { try? await Task.sleep(for: .milliseconds(200)) }
            do {
                shared = try await model.sharedChat(token)
            } catch BeepError.NotFound {
                gone = true
            } catch {
                self.error = describe(error)
            }
        }
    }

    /// Only the words are shared, so each entry is a plain bubble or reply.
    private func messages(_ shared: SharedSession) -> [Message] {
        shared.entries.enumerated().compactMap { index, entry in
            switch entry {
            case .user(let content, _, _): Message(id: "u\(index)", role: .user, text: content)
            case .assistant(let content, _, _, _, _, _): Message(id: "a\(index)", role: .assistant, text: content)
            default: nil
            }
        }
    }
}

extension Session: Identifiable {}
