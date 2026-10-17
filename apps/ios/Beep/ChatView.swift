import SwiftUI

struct ChatView: View {
    @Environment(AppModel.self) private var model
    let back: () -> Void
    @State private var draft = ""
    @State private var photos: [Data] = []
    @State private var picking = false
    @State private var camera = false
    /// Messages already on screen when the chat opened, or already shown; these do not spring in.
    @State private var settled: Set<String> = []
    @State private var squish = false
    @FocusState private var typing: Bool
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        VStack(spacing: 0) {
            chatBar
            ScrollViewReader { scroller in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 18) {
                        if model.chat.messages.isEmpty {
                            EmptyChat { model.send($0) } fill: { draft = $0; typing = true }
                        }
                        ForEach(Array(model.chat.messages.enumerated()), id: \.element.id) { index, message in
                            MessageView(message: message, live: message.id == "assistant-\(model.chat.runId)",
                                        quote: quote)
                                // A reply sits close under its own prompt; turns keep the full gap.
                                .padding(.top, index > 0 && message.role == .assistant
                                         && model.chat.messages[index - 1].role == .user ? -12 : 0)
                                .popIn(play: !settled.contains(message.id),
                                       anchor: message.role == .user ? .bottomTrailing : .bottomLeading)
                                .onAppear { settled.insert(message.id) }
                        }
                        if model.chat.busy, !replyStarted {
                            working
                                .popIn(anchor: .bottomLeading)
                                .transition(.opacity)
                        }
                        if !model.chat.error.isEmpty, !model.chat.busy {
                            ErrorRow(text: model.chat.error) { model.retry() }.unfold()
                        } else if !model.chat.error.isEmpty {
                            Text(model.chat.error).font(.body(13)).foregroundStyle(Mint.muted).unfold()
                        }
                        Color.clear.frame(height: 1).id("end")
                    }
                    .padding(18)
                    .animation(.bouncy, value: model.chat.busy)
                    .animation(.bouncy, value: replyStarted)
                    .animation(.easeOut(duration: 0.2), value: model.chat.error)
                }
                // Open at the latest message instead of scrolling down from the top.
                .defaultScrollAnchor(.bottom)
                .scrollDismissesKeyboard(.interactively)
                .onChange(of: model.chat.messages) { old, new in
                    // A new message glides into view; streamed text just keeps the end in sight.
                    let glide = old.count != new.count && !reduceMotion
                    withAnimation(glide ? .smooth : nil) { scroller.scrollTo("end", anchor: .bottom) }
                }
                .onChange(of: model.chat.id, initial: true) {
                    settled = Set(model.chat.messages.map(\.id))
                    scroller.scrollTo("end", anchor: .bottom)
                }
            }
            composer
        }
        .background(Mint.page)
        .toolbar(.hidden, for: .navigationBar)
        .onAppear {
            model.onScreen = model.chat.id
            #if DEBUG
            if UserDefaults.standard.string(forKey: "open") == "photos" { picking = true }
            #endif
        }
        .onChange(of: model.chat.id) { model.onScreen = model.chat.id }
        .onDisappear { if model.onScreen == model.chat.id { model.onScreen = nil } }
        .sheet(isPresented: $picking) {
            PhotoSheet(room: max(0, ChatPhotos.limit - photos.count)) { picked in
                picking = false
                photos += picked
            } camera: {
                picking = false
                Task {
                    try? await Task.sleep(for: .milliseconds(350))
                    camera = true
                }
            }
        }
        .fullScreenCover(isPresented: $camera) {
            ChatCamera(room: max(0, ChatPhotos.limit - photos.count)) { shots in
                camera = false
                photos += shots
            }
        }
    }

    /// Whether the run's reply has text yet; until then the working row stands in for it.
    private var replyStarted: Bool {
        guard let last = model.chat.messages.last else { return false }
        return last.role == .assistant && !last.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    /// Hopping dots and a changing verb, the tail of the model's thinking, and where the reply will go.
    private var working: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 10) {
                PulsingDots()
                StatusLine()
            }
            .padding(.leading, 4)
            if !model.chat.thinking.isEmpty {
                ThinkingPreview(text: model.chat.thinking).padding(.top, 8).padding(.trailing, 48).unfold()
            }
            ReplySkeleton().padding(.top, 12).padding(.trailing, 48)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .animation(.easeOut(duration: 0.22), value: model.chat.thinking.isEmpty)
    }

    /// Back to the list, Beep's avatar, the chat's name and the connection dot.
    private var chatBar: some View {
        HStack(spacing: 8) {
            Button {
                typing = false
                back()
            } label: {
                Image(systemName: "arrow.left").font(.system(size: 18, weight: .semibold))
                    .foregroundStyle(Mint.ink).frame(width: 44, height: 44).contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Chats")
            ChatAvatar(text: "b", fill: Mint.brand, size: 38, light: true)
            VStack(alignment: .leading, spacing: 2) {
                Text(model.chat.title.isEmpty ? "Beep" : model.chat.title)
                    .font(.display(20)).foregroundStyle(Mint.ink).lineLimit(1)
                HStack(spacing: 6) {
                    ConnectionDot(connected: model.connected)
                    if !model.connected {
                        Text("Tap to reconnect").font(.body(12, .medium)).foregroundStyle(Mint.muted)
                    } else if model.chat.busy {
                        Text("typing…").font(.body(12)).foregroundStyle(Mint.muted)
                    }
                }
                .contentShape(Rectangle())
                .onTapGesture { if !model.connected { model.connect() } }
            }
            Spacer(minLength: 0)
        }
        .padding(.leading, 6)
        .padding(.trailing, 16)
        .padding(.vertical, 6)
        .background(Mint.page)
    }

    private var composer: some View {
        let empty = draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && photos.isEmpty
        let prompt = photos.count > 1 ? "Ask about the photos..." : photos.isEmpty ? "Ask anything..." : "Ask about the photo..."
        return VStack(alignment: .leading, spacing: 0) {
            if !photos.isEmpty { thumbnails.unfold() }
            HStack(alignment: .bottom, spacing: 8) {
                HStack(alignment: .bottom, spacing: 4) {
                    TextField("", text: $draft, prompt: Text(prompt).foregroundStyle(Mint.dim), axis: .vertical)
                        .font(.body(15))
                        .foregroundStyle(Mint.ink)
                        .tint(Mint.ink)
                        .lineLimit(1...6)
                        .focused($typing)
                        .padding(.vertical, 14)
                    if photos.count < ChatPhotos.limit {
                        Button {
                            typing = false
                            picking = true
                        } label: {
                            Image(systemName: "photo").font(.system(size: 18)).foregroundStyle(Mint.muted)
                                .frame(width: 36, height: 36).contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .padding(.bottom, 7)
                        .accessibilityLabel("Add a photo")
                    }
                }
                .padding(.leading, 16).padding(.trailing, 6)
                .frame(minHeight: 50)
                .chunky()
                if model.chat.busy {
                    Button { model.stop() } label: { Image(systemName: "stop.fill").foregroundStyle(Mint.page) }
                        .buttonStyle(ChunkyButton(fill: Mint.ink, size: 50, radius: 25))
                        .foregroundStyle(Mint.page)
                        .accessibilityLabel("Stop reply")
                        .transition(.scale(scale: 0.6).combined(with: .opacity))
                } else {
                    Button {
                        model.send(draft, images: photos)
                        draft = ""
                        photos = []
                        // A quick squish on tap so sending feels physical.
                        guard !reduceMotion else { return }
                        withAnimation(.easeIn(duration: 0.07)) { squish = true } completion: {
                            withAnimation(.interpolatingSpring(stiffness: 600, damping: 2 * 0.4 * sqrt(600))) { squish = false }
                        }
                    } label: { Image(systemName: "arrow.up").foregroundStyle(empty ? Mint.ink : .white) }
                        .buttonStyle(ChunkyButton(fill: empty ? Mint.surface : Mint.brand, size: 50, radius: 25))
                        .opacity(empty ? 0.5 : 1)
                        .scaleEffect(squish ? 0.82 : 1)
                        .disabled(empty)
                        .accessibilityLabel("Send")
                        .transition(.scale(scale: 0.6).combined(with: .opacity))
                }
            }
            .animation(.bouncy, value: model.chat.busy)
            .animation(.easeOut(duration: 0.15), value: empty)
            .padding(.leading, 14)
            .padding(.trailing, 16)
            .padding(.top, 8)
            .padding(.bottom, 12)
        }
        .background(Mint.page)
        .animation(.bouncy, value: photos.count)
    }

    /// The photos waiting to go with the next message, each with a remove button.
    private var thumbnails: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(Array(photos.enumerated()), id: \.offset) { index, data in
                    ZStack(alignment: .topTrailing) {
                        PhotoThumb(data: data).frame(width: 64, height: 64)
                            .clipShape(RoundedRectangle(cornerRadius: 12))
                            .overlay(RoundedRectangle(cornerRadius: 12).stroke(Mint.ledge, lineWidth: 1.5))
                        Button { photos.remove(at: index) } label: {
                            Image(systemName: "xmark").font(.system(size: 9, weight: .bold)).foregroundStyle(Mint.page)
                                .frame(width: 20, height: 20).background(Circle().fill(Mint.ink))
                        }
                        .buttonStyle(.plain)
                        .padding(3)
                        .accessibilityLabel("Remove photo \(index + 1)")
                    }
                    .popIn()
                    .transition(.scale(scale: 0.6).combined(with: .opacity))
                }
            }
            .padding(.horizontal, 14).padding(.top, 8)
        }
    }

    /// Puts the quoted text above whatever is already typed.
    private func quote(_ text: String) {
        let lines = text.trimmingCharacters(in: .whitespacesAndNewlines)
            .split(separator: "\n", omittingEmptySubsequences: false).prefix(4)
        let quoted = lines.map { "> " + $0 }.joined(separator: "\n") + "\n\n"
        draft = quoted + draft.trimmingCharacters(in: .whitespacesAndNewlines)
        typing = true
    }
}

private struct ErrorRow: View {
    let text: String
    let retry: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: "exclamationmark.circle").foregroundStyle(Mint.error)
            Text(text).font(.body(13)).foregroundStyle(Mint.ink)
            Spacer(minLength: 0)
            Button("Retry", action: retry).buttonStyle(TextAction(color: Mint.error))
        }
        .padding(.leading, 12).padding(.trailing, 4).padding(.vertical, 4)
        .background(RoundedRectangle(cornerRadius: 12).fill(Mint.error.opacity(0.1)))
    }
}

private struct EmptyChat: View {
    @Environment(AppModel.self) private var model
    let send: (String) -> Void
    let fill: (String) -> Void
    @State private var loading = false
    @State private var spin = false

    /// Templates until the server's starters arrive; the last two only fill the composer.
    private static let templates: [(title: String, prompt: String, complete: Bool)] = [
        ("Remember something", "Remember that I prefer short, practical answers.", true),
        ("Search the web", "Search the web for ", false),
        ("Set a reminder", "Remind me to ", false),
    ]

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            (Text("A little help.\n") + Text("Right here.").foregroundColor(Mint.ok))
                .font(.display(40)).foregroundStyle(Mint.ink)
                .lineSpacing(0)
                .popIn(anchor: .bottomLeading)
                .padding(.top, 24)
            Text("Find an answer. Untangle a thought.\nKeep something worth remembering.")
                .font(.body(15)).foregroundStyle(Mint.muted).lineSpacing(5)
                .padding(.top, 12)
                .padding(.bottom, 28)
            let items = model.starters.isEmpty ? Self.templates
                : model.starters.prefix(3).map { (title: $0.title, prompt: $0.prompt, complete: true) }
            VStack(spacing: 12) {
                ForEach(Array(items.enumerated()), id: \.offset) { index, item in
                    Button { item.complete ? send(item.prompt) : fill(item.prompt) } label: {
                        HStack(alignment: .top, spacing: 12) {
                            Image(systemName: starterIcon(item.title + " " + item.prompt))
                                .font(.system(size: 16, weight: .medium)).foregroundStyle(Mint.ink)
                                .frame(width: 34, height: 34)
                                .background(RoundedRectangle(cornerRadius: 12).fill(Mint.tints[index % 3]))
                            Text(item.title).font(.body(15, .medium)).foregroundStyle(Mint.ink)
                                .multilineTextAlignment(.leading)
                                .frame(maxWidth: .infinity, minHeight: 34, alignment: .leading)
                            Image(systemName: "arrow.up.right").font(.system(size: 13, weight: .medium))
                                .foregroundStyle(Mint.muted).padding(.top, 10)
                        }
                        .padding(12)
                        .chunky()
                    }
                    .buttonStyle(.plain)
                    .id(item.title)
                    .popIn(delay: 0.06 * Double(index + 1))
                }
            }
            if !model.starters.isEmpty {
                Button {
                    loading = true
                    Task {
                        await model.loadStarters(refresh: true)
                        loading = false
                    }
                } label: {
                    Label {
                        Text("New ideas")
                    } icon: {
                        Image(systemName: "arrow.clockwise")
                            .rotationEffect(.degrees(spin ? 360 : 0))
                    }
                    .font(.body(13))
                }
                .disabled(loading)
                .onChange(of: loading) {
                    if loading {
                        withAnimation(.linear(duration: 0.9).repeatForever(autoreverses: false)) { spin = true }
                    } else {
                        withTransaction(Transaction(animation: nil)) { spin = false }
                    }
                }
                .buttonStyle(TextAction(color: Mint.muted))
                .frame(maxWidth: .infinity)
                .padding(.top, 4)
            }
        }
    }
}

/// The same icon choice as the Android starter cards.
private func starterIcon(_ text: String) -> String {
    let t = text.lowercased()
    func has(_ words: String...) -> Bool { words.contains { t.contains($0) } }
    if has("remind", "alarm", "timer") { return "alarm" }
    if has("remember", "note", "save") { return "bookmark" }
    if has("mail", "inbox") { return "envelope" }
    if has("calendar", "meeting", "schedule", "week", "today", "tomorrow") { return "calendar" }
    if has("map", "near", "place", "restaurant", "route", "where") { return "mappin.and.ellipse" }
    if has("weather", "rain", "forecast") { return "sun.max" }
    if has("search", "news", "latest", "find", "look up") { return "magnifyingglass" }
    if has("plan", "idea", "brainstorm", "write", "draft") { return "lightbulb" }
    return "sparkles"
}

/// The tail of the model's reasoning while it thinks, the oldest lines fading out at the top.
private struct ThinkingPreview: View {
    let text: String

    var body: some View {
        Text(String(text.trimmingCharacters(in: .whitespacesAndNewlines).suffix(1500)))
            .font(.body(13)).foregroundStyle(Mint.muted)
            .frame(maxWidth: .infinity, alignment: .leading)
            .fixedSize(horizontal: false, vertical: true)
            .frame(maxHeight: 140, alignment: .bottom)
            .clipped()
            .mask(LinearGradient(stops: [.init(color: .clear, location: 0), .init(color: .black, location: 0.2)],
                                 startPoint: .top, endPoint: .bottom))
            .padding(.horizontal, 12).padding(.vertical, 8)
            .background(RoundedRectangle(cornerRadius: 12).fill(Mint.surface))
    }
}

struct MessageView: View {
    @Environment(AppModel.self) private var model
    let message: Message
    let live: Bool
    var quote: (String) -> Void = { _ in }
    @State private var showThoughts = false
    @State private var showAllTools = false
    @State private var copied = false

    var body: some View {
        switch message.role {
        case .user:
            userBubble
                .chunky(Mint.user, shape: UnevenRoundedRectangle(
                    topLeadingRadius: 18, bottomLeadingRadius: 18, bottomTrailingRadius: 6, topTrailingRadius: 18))
                .padding(.leading, 48)
                .frame(maxWidth: .infinity, alignment: .trailing)
        case .scheduled:
            HStack(spacing: 6) {
                Image(systemName: "clock").font(.system(size: 11))
                Text(message.text.isEmpty ? "Scheduled" : "Scheduled · \(message.text)").lineLimit(1)
            }
            .font(.body(11)).foregroundStyle(Mint.dim)
            .frame(maxWidth: .infinity)
        case .assistant:
            VStack(alignment: .leading, spacing: 0) {
                if !message.reasoning.isEmpty { thoughts.padding(.bottom, 4) }
                tools
                ForEach(message.approvals, id: \.queryId) { prompt in
                    ApprovalCard(prompt: prompt).padding(.bottom, 8)
                }
                if !message.text.isEmpty {
                    Typewriter(text: message.text, streaming: live) { MarkdownText(text: $0) }
                }
                // The mock shows a saved place under the reply that names it.
                ForEach(message.tools.compactMap(\.savedPlace), id: \.id) { place in
                    PlaceCard(place: place) { model.destination = .placeRoutine(place.id) }
                        .clipShape(RoundedRectangle(cornerRadius: 18))
                        .card()
                        .padding(.top, 10)
                }
                ForEach(message.tools.filter { $0.name == "get_route" && $0.done && $0.success }) { call in
                    RouteCardView(callId: call.id).padding(.top, 10)
                }
                ForEach(message.tools.filter { $0.name == "show_products" }) { call in
                    let products = call.products
                    if !products.isEmpty { ProductRow(products: products).padding(.top, 10) }
                }
                if !message.text.isEmpty, !live { actions.transition(.opacity) }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .animation(.easeOut(duration: 0.2), value: live)
            .animation(.bouncy, value: message.approvals.count)
        }
    }

    /// The text, with any pictures above it in a 220 point wide block as in the mock.
    @ViewBuilder private var userBubble: some View {
        let text = Text(message.text).font(.body(15)).foregroundStyle(Mint.ink).textSelection(.enabled)
        if message.pictures.isEmpty {
            text.padding(.horizontal, 14).padding(.vertical, 10)
        } else {
            VStack(alignment: .leading, spacing: 0) {
                PictureGrid(pictures: message.pictures)
                if !message.text.isEmpty {
                    text.padding(.horizontal, 10).padding(.top, 8).padding(.bottom, 6)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            .padding(4)
            .frame(width: 220)
        }
    }

    private var thoughts: some View {
        VStack(alignment: .leading, spacing: 0) {
            Button { withAnimation(.snappy) { showThoughts.toggle() } } label: {
                HStack(spacing: 2) {
                    Text("Thoughts").font(.body(12, .medium))
                    Image(systemName: "chevron.down").font(.system(size: 11, weight: .semibold))
                        .rotationEffect(.degrees(showThoughts ? 180 : 0))
                }
                .foregroundStyle(Mint.muted)
                .padding(.horizontal, 4)
                .frame(minHeight: 40)
            }
            .buttonStyle(.plain)
            if showThoughts {
                MarkdownText(text: message.reasoning, size: 13, color: Mint.muted)
                    .padding(.horizontal, 12).padding(.vertical, 10)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(RoundedRectangle(cornerRadius: 12).fill(Mint.surface))
                    .padding(.bottom, 6)
                    .unfold()
            }
        }
    }

    /// While a reply runs only the latest three chips show and the earlier ones fold
    /// into one line; once it is done a long run folds away entirely.
    @ViewBuilder private var tools: some View {
        let calls = message.tools.filter { !quietTools.contains($0.name) }
        if !calls.isEmpty {
            let finished = !live && calls.allSatisfy(\.done)
            let folded = finished && calls.count > 3 ? calls.count : max(0, calls.count - 3)
            let hidden = showAllTools ? 0 : folded
            VStack(alignment: .leading, spacing: 6) {
                if folded > 0 {
                    FoldedTools(calls: Array(calls.prefix(folded)), finished: folded == calls.count,
                                open: showAllTools) { withAnimation(.snappy) { showAllTools.toggle() } }
                        .unfold()
                }
                ForEach(Array(calls.enumerated()), id: \.element.id) { index, call in
                    if index >= hidden { ToolChip(call: call, tint: Mint.tints[index % 3]).unfold() }
                }
            }
            .padding(.bottom, 8)
            .animation(.bouncy, value: calls.count)
            .animation(.bouncy, value: folded)
        }
    }

    private var actions: some View {
        HStack(spacing: 0) {
            Button {
                UIPasteboard.general.string = message.text
                copied = true
                Task {
                    try? await Task.sleep(for: .seconds(1.8))
                    copied = false
                }
            } label: {
                HStack(spacing: 6) {
                    Image(systemName: copied ? "checkmark" : "doc.on.doc").font(.system(size: 12))
                        .foregroundStyle(copied ? Mint.ok : Mint.muted)
                    Text(copied ? "Copied" : "Copy reply").font(.body(11))
                }
            }
            Button { quote(message.text) } label: {
                HStack(spacing: 6) {
                    Image(systemName: "quote.opening").font(.system(size: 12))
                    Text("Quote").font(.body(11))
                }
            }
        }
        .buttonStyle(TextAction(color: Mint.muted))
        .padding(.leading, -8)
        .padding(.top, 2)
    }
}

/// The folded part of a run of tools: their icons stacked, a count and a chevron.
struct FoldedTools: View {
    let calls: [ToolCall]
    let finished: Bool
    let open: Bool
    let toggle: () -> Void

    var body: some View {
        Button(action: toggle) {
            HStack(spacing: 6) {
                HStack(spacing: -6) {
                    ForEach(Array(calls.prefix(4).enumerated()), id: \.offset) { index, call in
                        Text(toolInitial(call.name))
                            .font(.body(9, .semibold)).foregroundStyle(Mint.ink)
                            .frame(width: 20, height: 20)
                            .background(Circle().fill(Mint.tints[index % 3]))
                            .overlay(Circle().stroke(Mint.page, lineWidth: 2))
                    }
                }
                Text(summary).font(.body(12)).foregroundStyle(Mint.muted)
                    .contentTransition(.numericText())
                    .animation(.snappy, value: summary)
                Image(systemName: "chevron.down").font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(Mint.muted)
                    .rotationEffect(.degrees(open ? 180 : 0))
            }
            .padding(.leading, 4).padding(.trailing, 8).padding(.vertical, 3)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    private var summary: String {
        let count = calls.count
        if !finished { return "+\(count) \(count == 1 ? "tool" : "tools")" }
        let failed = calls.filter { $0.done && !$0.success }.count
        return "\(count) \(count == 1 ? "tool" : "tools")" + (failed > 0 ? ", \(failed) failed" : "")
    }
}

/// A tool call as a chip. A running one springs in and its icon wiggles; when it
/// finishes the done mark is stamped on. Chips already done when shown stay still.
struct ToolChip: View {
    let call: ToolCall
    var tint: Color = Mint.tints[1]
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var sawRunning: Bool?
    @State private var wiggle = false
    @State private var stamped = false

    var body: some View {
        let still = reduceMotion || !(sawRunning ?? !call.done)
        HStack(spacing: 8) {
            Text(toolInitial(call.name))
                .font(.body(11, .semibold))
                .frame(width: 26, height: 26)
                .background(Circle().fill(Mint.surface))
                .rotationEffect(.degrees(!call.done && !still ? (wiggle ? 12 : -12) : 0))
            Text(call.done ? doneLabel(call.label) : call.label)
                .font(.body(13, .medium)).lineLimit(1)
                .contentTransition(.opacity)
            ZStack {
                if call.done {
                    let on = stamped || still
                    Image(systemName: call.success ? "checkmark" : "xmark")
                        .font(.system(size: 9, weight: .heavy)).foregroundStyle(.white)
                        .frame(width: 18, height: 18)
                        .background(Circle().fill(call.success ? Mint.ok : Mint.error))
                        .scaleEffect(on ? 1 : 1.8)
                        .rotationEffect(.degrees(on ? 0 : -14))
                        .opacity(on ? 1 : 0)
                } else {
                    ProgressView().controlSize(.mini).tint(Mint.ink).frame(width: 13, height: 13)
                }
            }
            .frame(width: 18, height: 18)
        }
        .foregroundStyle(Mint.ink)
        .padding(.leading, 5).padding(.trailing, 12).padding(.vertical, 5)
        .chunky(tint, shape: Capsule())
        .popIn(play: !call.done, anchor: .leading)
        .onAppear { if sawRunning == nil { sawRunning = !call.done } }
        .task(id: call.done) {
            let moving = !reduceMotion && (sawRunning ?? !call.done)
            if call.done {
                withTransaction(Transaction(animation: nil)) { wiggle = false }
                if moving { withAnimation(.stamp) { stamped = true } }
            } else if moving {
                withAnimation(.linear(duration: 0.26).repeatForever(autoreverses: true)) { wiggle = true }
            }
        }
    }
}

struct ApprovalCard: View {
    @Environment(AppModel.self) private var model
    let prompt: ApprovalPrompt

    var body: some View {
        if let answer = prompt.answer {
            Label(answerText(answer), systemImage: "shield")
                .font(.body(12, .medium)).foregroundStyle(Mint.muted)
        } else {
            VStack(alignment: .leading, spacing: 0) {
                HStack(spacing: 10) {
                    Text(String(prompt.approval.app.prefix(1)).uppercased())
                        .font(.body(14, .semibold))
                        .frame(width: 32, height: 32)
                        .background(RoundedRectangle(cornerRadius: 9).fill(Mint.surface))
                        .overlay(RoundedRectangle(cornerRadius: 9).stroke(Mint.ledge, lineWidth: 2))
                    VStack(alignment: .leading, spacing: 2) {
                        Text("\(prompt.approval.action) with \(prompt.approval.app)?").font(.body(15, .semibold))
                        Text("\(prompt.approval.app) asks before it does this").font(.body(12)).foregroundStyle(Mint.muted)
                    }
                }
                .padding(.horizontal, 14).padding(.vertical, 12)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Mint.tints[2])
                Rectangle().fill(Mint.ledge).frame(height: 1.5)
                Grid(alignment: .leading, horizontalSpacing: 8, verticalSpacing: 4) {
                    ForEach(Array(prompt.approval.fields.enumerated()), id: \.offset) { _, field in
                        GridRow {
                            Text(field.label).foregroundStyle(Mint.muted)
                            Text(field.value).lineLimit(6)
                        }
                    }
                }
                .font(.body(13))
                .padding(.horizontal, 14).padding(.top, 12)
                HStack(spacing: 8) {
                    Button("Allow") { model.answer(prompt, .allow) }
                        .font(.body(14, .semibold)).foregroundStyle(.white)
                        .padding(.horizontal, 20).frame(minHeight: 44)
                        .chunky(Mint.ok, radius: 12)
                    Button("Always") { model.answer(prompt, .always) }.buttonStyle(PillButton())
                    Spacer()
                    Button("Deny") { model.answer(prompt, .deny) }
                        .font(.body(14, .medium)).foregroundStyle(Mint.error)
                }
                .padding(14)
            }
            .foregroundStyle(Mint.ink)
            .chunky()
        }
    }

    private func answerText(_ answer: ApprovalDecision) -> String {
        switch answer {
        case .allow: "Allowed once"
        case .always: "Always allowed for \(prompt.approval.app)"
        case .deny: "Denied"
        }
    }
}

/// Replies as markdown: paragraphs, bullets, headings, code and tables, with inline styling.
struct MarkdownText: View {
    let text: String
    var size: CGFloat = 15
    var color: Color = Mint.ink

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            ForEach(Array(blocks.enumerated()), id: \.offset) { _, block in
                switch block {
                case .bullet(let line):
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text("•")
                        inline(line)
                    }
                case .heading(let line):
                    inline(line).fontWeight(.bold)
                case .code(let code):
                    Text(code)
                        .font(.system(size: size - 2, design: .monospaced))
                        .padding(10)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(RoundedRectangle(cornerRadius: 10).fill(Mint.tints[1]))
                case .table(let table):
                    tableView(table)
                case .rule:
                    Rectangle().fill(Mint.soft).frame(height: 1).padding(.vertical, 6)
                case .products(let products):
                    ProductRow(products: products).padding(.vertical, 4)
                case .paragraph(let line):
                    inline(line)
                }
            }
        }
        .font(.body(size))
        .foregroundStyle(color)
        .lineSpacing(3)
        .textSelection(.enabled)
    }

    private enum Block {
        case paragraph(String), bullet(String), heading(String), code(String), table(Table), rule
        case products([Product])
    }

    private struct Table {
        let header: [String]
        let rows: [[String]]
    }

    /// A run of `| a | b |` lines is a table when it has a header, a `---` line and a row.
    private static func table(_ lines: [String]) -> Table? {
        guard lines.count >= 3, lines[1].contains("---") else { return nil }
        let cells = { (line: String) -> [String] in
            // Keep empty cells so later ones stay in their column; only the outer pipes go.
            let line = line.trimmingCharacters(in: .whitespaces)
            var parts = line.split(separator: "|", omittingEmptySubsequences: false)
            if line.hasPrefix("|") { parts.removeFirst() }
            if line.hasSuffix("|"), !parts.isEmpty { parts.removeLast() }
            return parts.map { $0.trimmingCharacters(in: .whitespaces) }
        }
        let header = cells(lines[0])
        let rows = lines.dropFirst(2).map { row in
            let row = cells(row)
            return Array(row.prefix(header.count)) + Array(repeating: "", count: max(0, header.count - row.count))
        }
        return Table(header: header, rows: rows)
    }

    private func tableView(_ table: Table) -> some View {
        ScrollView(.horizontal, showsIndicators: false) {
            Grid(alignment: .topLeading, horizontalSpacing: 0, verticalSpacing: 0) {
                GridRow {
                    ForEach(Array(table.header.enumerated()), id: \.offset) { _, cell in
                        tableCell(cell, lines: 2).fontWeight(.semibold).background(Mint.tints[1])
                    }
                }
                ForEach(Array(table.rows.enumerated()), id: \.offset) { _, row in
                    Rectangle().fill(Mint.soft).frame(height: 1).gridCellUnsizedAxes(.horizontal)
                    GridRow {
                        ForEach(Array(row.enumerated()), id: \.offset) { _, cell in
                            tableCell(cell, lines: 4)
                        }
                    }
                }
            }
            .background(Mint.surface)
            .clipShape(RoundedRectangle(cornerRadius: 10))
            .overlay(RoundedRectangle(cornerRadius: 10).stroke(Mint.soft, lineWidth: 1))
            .padding(1)
        }
        .font(.body(size - 3))
        .lineSpacing(1)
    }

    private func tableCell(_ text: String, lines: Int) -> some View {
        inline(text)
            .lineLimit(lines)
            .frame(minWidth: 90, maxWidth: 220, maxHeight: .infinity, alignment: .topLeading)
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
    }

    private var blocks: [Block] {
        var out: [Block] = []
        var code: [String]?
        var fence = ""
        var rows: [String] = []
        /// A product block joins the row before it; one that does not parse stays code.
        func closeFence(_ lines: [String]) {
            let body = lines.joined(separator: "\n")
            guard fence == "product", let product = Product.parse(body) else {
                out.append(.code(body))
                return
            }
            if case .products(let row) = out.last {
                out[out.count - 1] = .products(row + [product])
            } else {
                out.append(.products([product]))
            }
        }
        func flushRows() {
            if let table = Self.table(rows) {
                out.append(.table(table))
            } else {
                out += rows.map { .paragraph($0) }
            }
            rows = []
        }
        for raw in text.components(separatedBy: "\n") {
            let line = raw.trimmingCharacters(in: .whitespaces)
            if code == nil, line.count > 1, line.hasPrefix("|"), line.hasSuffix("|") {
                rows.append(line)
                continue
            }
            if !rows.isEmpty { flushRows() }
            if line.hasPrefix("```") {
                if let lines = code {
                    closeFence(lines)
                    code = nil
                } else {
                    fence = line.dropFirst(3).trimmingCharacters(in: .whitespaces).lowercased()
                    code = []
                }
                continue
            }
            if code != nil {
                code?.append(raw)
                continue
            }
            if line.isEmpty { continue }
            if line.count >= 3, let mark = line.first, "-*_".contains(mark), line.allSatisfy({ $0 == mark }) {
                out.append(.rule)
            } else if line.hasPrefix("- ") || line.hasPrefix("* ") {
                out.append(.bullet(String(line.dropFirst(2))))
            } else if line.hasPrefix("#") {
                out.append(.heading(line.drop(while: { $0 == "#" }).trimmingCharacters(in: .whitespaces)))
            } else {
                out.append(.paragraph(line))
            }
        }
        if !rows.isEmpty { flushRows() }
        if let lines = code {
            // A product block still streaming in waits for its closing fence.
            if fence != "product" { out.append(.code(lines.joined(separator: "\n"))) }
        }
        return out
    }

    private func inline(_ line: String) -> Text {
        let options = AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace)
        if let styled = try? AttributedString(markdown: line, options: options) { return Text(styled) }
        return Text(line)
    }
}
