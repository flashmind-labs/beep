import SwiftUI

struct ChatView: View {
    @Environment(AppModel.self) private var model
    let menu: () -> Void
    let newChat: () -> Void
    /// Opens the workspace of the research on screen, on a tab or the one that fits.
    var workspace: (WorkspaceTab?) -> Void = { _ in }
    @State private var research = ResearchWatch()
    @State private var paper = PaperShare()
    @State private var draft = ""
    /// Replaced on each send, see `clearDraft`.
    @State private var field = 0
    @State private var photos: [Data] = []
    @State private var files: [SentFile] = []
    @State private var importing = false
    @State private var fileError = ""
    @State private var picking = false
    @State private var camera = false
    /// Messages already on screen when the chat opened, or already shown; these do not spring in.
    @State private var settled: Set<String> = []
    @State private var squish = false
    /// Follows the reply until the user scrolls up; reaching the end again resumes it.
    @State private var follow = true
    /// Whether the chat is taller than the screen, which brings up the prompt map.
    @State private var scrolls = false
    @State private var voice = VoiceInput()
    @FocusState private var typing: Bool
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        VStack(spacing: 0) {
            chatBar
            ScrollViewReader { scroller in
                let breaks = dayBreaks(model.chat.messages)
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 18) {
                        if model.chat.messages.isEmpty {
                            EmptyChat { follow = true; model.send($0) } fill: { draft = $0; typing = true }
                        }
                        ForEach(Array(model.chat.messages.enumerated()), id: \.element.id) { index, message in
                            VStack(spacing: 12) {
                                if let day = breaks[message.id] { DayMarker(day: day) }
                                MessageView(message: message, live: message.id == "assistant-\(model.chat.runId)",
                                            latest: index == model.chat.messages.count - 1, quote: quote)
                                if message.id == newestReply?.id, !model.chat.busy {
                                    researchRow(after: message)
                                }
                            }
                            // A reply sits close under its own prompt; turns keep the full gap.
                            .padding(.top, index > 0 && message.role == .assistant
                                     && model.chat.messages[index - 1].role == .user ? -12 : 0)
                            .popIn(play: !settled.contains(message.id),
                                   anchor: message.role == .user ? .bottomTrailing : .bottomLeading)
                            .onAppear { settled.insert(message.id) }
                            .id(message.id)
                        }
                        if let asked = model.waitingWork[model.chat.id], !asked.isEmpty {
                            BackgroundQuestions(prompts: asked).popIn(anchor: .bottomLeading)
                        }
                        if model.chat.busy, !replyStarted {
                            working
                                .popIn(anchor: .bottomLeading)
                                .transition(.opacity)
                        }
                        if !model.chat.error.isEmpty, !model.chat.busy {
                            ErrorRow(text: model.chat.error, retry: { follow = true; model.retry() }) {
                                // A stopped turn may have no reply to carry the rerun menu, so it sits here too.
                                if let models = model.models, !models.choices.isEmpty,
                                    let prompt = model.chat.messages.last(where: { $0.role == .user }) {
                                    RerunButton(models: models, used: prompt.model.isEmpty ? nil : prompt.model,
                                                color: Mint.error, prominent: true) { choice, thinking in
                                        follow = true
                                        Task { await model.rerunPrompt(prompt, model: choice, thinking: thinking) }
                                    }
                                }
                            }
                            .unfold()
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
                .chatScrollAnchor()
                .followLatest($follow, scrolls: $scrolls)
                .scrollDismissesKeyboard(.interactively)
                .onChange(of: model.chat.messages) { old, new in
                    // Sending always brings the end back.
                    if new.count > old.count, new.last?.role == .user { follow = true }
                    guard follow else { return }
                    // A new message glides into view; streamed text just keeps the end in sight.
                    let glide = old.count != new.count && !reduceMotion
                    withAnimation(glide ? .smooth : nil) { scroller.scrollTo("end", anchor: .bottom) }
                }
                .onChange(of: research.live?.status) {
                    // The research's chips under the newest reply arrive after the chat scrolled to its end.
                    guard follow else { return }
                    withAnimation(reduceMotion ? nil : .smooth) { scroller.scrollTo("end", anchor: .bottom) }
                }
                .onChange(of: model.chat.id, initial: true) {
                    settled = Set(model.chat.messages.map(\.id))
                    follow = true
                    scroller.scrollTo("end", anchor: .bottom)
                }
                .overlay(alignment: .trailing) {
                    if scrolls {
                        PromptMap(messages: model.chat.messages) { id in
                            follow = false
                            withAnimation(reduceMotion ? nil : .smooth) { scroller.scrollTo(id, anchor: .top) }
                        }
                    }
                }
                .overlay(alignment: .bottom) {
                    if !follow, !model.chat.messages.isEmpty {
                        Button {
                            follow = true
                            withAnimation(reduceMotion ? nil : .smooth) { scroller.scrollTo("end", anchor: .bottom) }
                        } label: {
                            HStack(spacing: 6) {
                                Image(systemName: "arrow.down").font(.system(size: 13, weight: .semibold))
                                Text("Latest reply").font(.body(12, .medium))
                            }
                            .foregroundStyle(Mint.ink)
                            .padding(.horizontal, 14).padding(.vertical, 7)
                            .chunky(shape: Capsule())
                        }
                        .buttonStyle(.plain)
                        .padding(.vertical, 8)
                        .transition(.opacity.combined(with: .move(edge: .bottom)))
                    }
                }
                .animation(.snappy, value: follow)
            }
            // Live voice keeps the chat as it is and puts its controls where the composer sits.
            if model.live.active {
                LiveDock().transition(.opacity)
            } else {
                composer
            }
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
        .task(id: isResearch ? model.chat.id : "") { if isResearch { await research.follow(model, model.chat.id) } }
        .onChange(of: model.pendingQuote, initial: true) {
            // Quotes from the workspace pile up in the order they were picked, for one reply to them all.
            guard let text = model.pendingQuote else { return }
            model.pendingQuote = nil
            let quoted = text.trimmingCharacters(in: .whitespacesAndNewlines)
                .split(separator: "\n", omittingEmptySubsequences: false).map { "> " + $0 }.joined(separator: "\n") + "\n\n"
            let before = draft.trimmingCharacters(in: .whitespacesAndNewlines)
            draft = before.isEmpty ? quoted : before + "\n\n" + quoted
            typing = true
        }
        .paperShare(paper)
        .onChange(of: model.pendingDraft, initial: true) {
            // What was said in live voice and not sent.
            guard let text = model.pendingDraft else { return }
            model.pendingDraft = nil
            let before = draft.trimmingCharacters(in: .whitespacesAndNewlines)
            draft = before.isEmpty ? text : before + " " + text
        }
        .onChange(of: model.composeNext, initial: true) {
            guard model.composeNext else { return }
            model.composeNext = false
            typing = true
        }
        .closesForLive {
            typing = false
            voice.cancel()
            voice.dismissProblem()
            picking = false
            camera = false
            importing = false
            fileError = ""
        }
        .onDisappear {
            voice.cancel()
            if model.onScreen == model.chat.id { model.onScreen = nil }
        }
        .fullScreenCover(isPresented: Binding(get: { model.browserOpen }, set: { model.browserOpen = $0 })) {
            BrowserView(close: { model.browserOpen = false })
        }
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
            } files: {
                picking = false
                Task {
                    try? await Task.sleep(for: .milliseconds(350))
                    importing = true
                }
            }
        }
        .fileImporter(isPresented: $importing, allowedContentTypes: [.item], allowsMultipleSelection: true) { result in
            guard case .success(let urls) = result else { return }
            var room = SentFile.limit - files.reduce(0) { $0 + $1.size }
            var tooBig = false
            // A queued message put back can leave more files than the limit; prefix traps below zero.
            for url in urls.prefix(max(0, ChatPhotos.limit - files.count)) {
                guard let file = try? SentFile.attach(url) else { fileError = "Couldn't read that file"; continue }
                if file.size > room {
                    tooBig = true
                    if let local = file.local { try? FileManager.default.removeItem(at: local.deletingLastPathComponent()) }
                    continue
                }
                room -= file.size
                files.append(file)
            }
            if tooBig { fileError = "Files can be 5 MB in all" }
        }
        .alert(fileError, isPresented: Binding(get: { !fileError.isEmpty }, set: { if !$0 { fileError = "" } })) {
            Button("OK", role: .cancel) {}
        }
        .alert(voice.problem?.text ?? "", isPresented: Binding(get: { voice.problem != nil }, set: { if !$0 { voice.dismissProblem() } })) {
            if voice.problem?.settings == true {
                Button("Open Settings") {
                    if let url = URL(string: UIApplication.openSettingsURLString) { UIApplication.shared.open(url) }
                }
            }
            Button(voice.problem?.settings == true ? "Not now" : "OK", role: .cancel) {}
        }
        .fullScreenCover(isPresented: $camera) {
            ChatCamera(room: max(0, ChatPhotos.limit - photos.count)) { shots in
                camera = false
                photos += shots
            }
        }
    }

    /// Whether the chat on screen is a research, whose lead answers in it.
    private var isResearch: Bool { model.researches.contains(model.chat.id) }
    private var live: ResearchLive? { isResearch ? research.live : nil }
    /// The reply with text that ends the chat; a turn of only tool calls after it does not count.
    private var newestReply: Message? {
        guard let last = model.chat.messages.lastIndex(where: { $0.role == .assistant && !$0.text.isEmpty }),
            !model.chat.messages[last...].contains(where: { $0.role == .user }) else { return nil }
        return model.chat.messages[last]
    }

    /// Under the newest reply: the research's next step.
    @ViewBuilder private func researchRow(after reply: Message) -> some View {
        // The lead's own suggested replies already say go.
        let offersReplies = reply.tools.contains { if case .replies = $0.card { true } else { false } }
        if let live, !(live.status == .planning && offersReplies) {
            ResearchActions(live: live, go: { follow = true; model.send("Go") }, paper: { workspace(.paper) },
                            share: { paper.share(model, model.chat.id) })
                .frame(maxWidth: .infinity, alignment: .leading)
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

    /// The menu for the chat list, the chat's name with the connection under it, and a new chat.
    private var chatBar: some View {
        HStack(spacing: 10) {
            Button {
                typing = false
                menu()
            } label: { Image(systemName: "line.3.horizontal") }
                .buttonStyle(ChunkyButton())
                .accessibilityLabel("Conversations")
            VStack(alignment: .leading, spacing: 2) {
                Text(model.chat.title.isEmpty ? "New conversation" : model.chat.title)
                    .font(.body(16, .semibold)).foregroundStyle(Mint.ink).lineLimit(1)
                if let live, model.connected {
                    ResearchHeadline(live: live)
                } else { HStack(spacing: 6) {
                    ConnectionDot(connected: model.connected)
                    Text(model.connected ? "Connected" : model.connecting ? "Connecting..." : "Tap to reconnect").font(.body(12, .medium)).foregroundStyle(Mint.muted)
                }
                .contentShape(Rectangle())
                .onTapGesture { if !model.connected { model.connect() } }
                }
            }
            Spacer(minLength: 0)
            if isResearch {
                Button {
                    typing = false
                    workspace(nil)
                } label: { Image(systemName: "doc.text") }
                    .buttonStyle(ChunkyButton())
                    .accessibilityLabel("Workspace")
            }
            if !model.live.active {
                Button {
                    typing = false
                    voice.cancel()
                    model.live.start(from: .app)
                } label: {
                    Image(systemName: "waveform").font(.system(size: 19, weight: .medium)).foregroundStyle(Mint.muted)
                        .frame(width: 44, height: 44).contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Talk live")
            }
            Button {
                typing = false
                newChat()
            } label: {
                Image(systemName: "plus").font(.system(size: 20)).foregroundStyle(Mint.muted)
                    .frame(width: 44, height: 44).contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("New chat")
        }
        .padding(.leading, 12)
        .padding(.trailing, 10)
        .padding(.vertical, 6)
        .background(Mint.page)
    }

    private var composer: some View {
        let empty = draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && photos.isEmpty && files.isEmpty
        let prompt = voice.listening ? "Speak now..." : photos.count > 1 ? "Ask about the photos..."
            : !photos.isEmpty ? "Ask about the photo..." : !files.isEmpty ? "Ask about the file..." : isResearch ? researchHint(live) : "Ask anything..."
        return VStack(alignment: .leading, spacing: 0) {
            if let ended = model.live.ended { liveEndedRow(ended).unfold() }
            if voice.listening { listeningRow.unfold() }
            if let live, live.running {
                ResearchProgressRow(live: live, open: { workspace(nil) }) {
                    let id = model.chat.id
                    Task { try? await model.stopResearch(id) }
                }
                .unfold()
            }
            if !model.chat.queued.isEmpty { queuedList.unfold() }
            if !photos.isEmpty { thumbnails.unfold() }
            if !files.isEmpty { fileChips.unfold() }
            HStack(alignment: .bottom, spacing: 8) {
                HStack(alignment: .bottom, spacing: 4) {
                    // As on Android, the photo button steps aside once there is something typed.
                    if !model.chat.busy, draft.isEmpty, photos.count < ChatPhotos.limit {
                        Button {
                            typing = false
                            picking = true
                        } label: {
                            Image(systemName: "photo").font(.system(size: 19)).foregroundStyle(Mint.muted)
                                .frame(width: 40, height: 40).contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .padding(.bottom, 5)
                        .accessibilityLabel("Add a photo or file")
                    }
                    TextField("", text: $draft, prompt: Text(prompt).foregroundStyle(Mint.dim), axis: .vertical)
                        .id(field)
                        .font(.body(15))
                        .foregroundStyle(Mint.ink)
                        .tint(Mint.ink)
                        .lineLimit(1...6)
                        .focused($typing)
                        .padding(.vertical, 14)
                        .padding(.leading, draft.isEmpty && !model.chat.busy ? 6 : 16)
                }
                .padding(.leading, 4).padding(.trailing, 12)
                .frame(minHeight: 50)
                .chunky()
                .blocksChatSwipe()
                VStack(spacing: 8) {
                    if model.chat.busy {
                        VStack(spacing: 8) {
                            if !empty {
                                Button {
                                    model.queue(draft, images: photos, files: files)
                                    clearDraft()
                                    photos = []
                                    files = []
                                } label: { Image(systemName: "clock") }
                                    .buttonStyle(ChunkyButton(fill: Mint.surface, size: 50, radius: 18))
                                    .accessibilityLabel("Queue message")
                                    .transition(.scale(scale: 0.6).combined(with: .opacity))
                            }
                            Button { model.stop() } label: { Image(systemName: "stop.fill").foregroundStyle(Mint.page) }
                                .buttonStyle(ChunkyButton(fill: Mint.ink, size: 50, radius: 18))
                                .foregroundStyle(Mint.page)
                                .accessibilityLabel("Stop reply")
                        }
                        .transition(.scale(scale: 0.6).combined(with: .opacity))
                    } else if voice.listening || (empty && voice.available) {
                        // Stays while listening, even once words fill the draft, so it can stop.
                        Button {
                            if voice.listening {
                                voice.stop()
                            } else {
                                typing = false
                                // What is said goes after what was typed before.
                                let before = draft.trimmingCharacters(in: .whitespacesAndNewlines)
                                Task { await voice.start { draft = before.isEmpty ? $0 : before + " " + $0 } }
                            }
                        } label: { Image(systemName: "mic") }
                            .buttonStyle(ChunkyButton(fill: voice.listening ? Mint.accent : Mint.surface, size: 50, radius: 18))
                            .background { if voice.listening { MicRing() } }
                            .accessibilityLabel(voice.listening ? "Stop listening" : "Voice input")
                            .transition(.scale(scale: 0.6).combined(with: .opacity))
                    } else {
                        Button {
                            voice.cancel()
                            model.live.dismissEnded()
                            model.send(draft, images: photos, files: files)
                            clearDraft()
                            photos = []
                            files = []
                            // A quick squish on tap so sending feels physical.
                            guard !reduceMotion else { return }
                            withAnimation(.easeIn(duration: 0.07)) { squish = true } completion: {
                                withAnimation(.interpolatingSpring(stiffness: 600, damping: 2 * 0.4 * sqrt(600))) { squish = false }
                            }
                        } label: { Image(systemName: "paperplane") }
                            .buttonStyle(ChunkyButton(fill: empty ? Mint.surface : Mint.accent, size: 50, radius: 18))
                            .opacity(empty ? 0.45 : 1)
                            .scaleEffect(squish ? 0.82 : 1)
                            .disabled(empty)
                            .accessibilityLabel("Send")
                            .transition(.scale(scale: 0.6).combined(with: .opacity))
                    }
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
        .animation(.bouncy, value: files.count)
    }

    /// Why live voice ended, over the composer until the next message or the cross.
    private func liveEndedRow(_ text: String) -> some View {
        HStack(spacing: 8) {
            Image(systemName: "mic").font(.system(size: 11, weight: .semibold))
            Text(text).font(.body(12)).frame(maxWidth: .infinity, alignment: .leading)
            Button { model.live.dismissEnded() } label: {
                Image(systemName: "xmark").font(.system(size: 11, weight: .semibold)).foregroundStyle(Mint.dim)
                    .frame(width: 32, height: 32).contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Dismiss")
        }
        .foregroundStyle(Mint.muted)
        .padding(.leading, 20).padding(.trailing, 12).padding(.top, 4)
    }

    /// The pulsing dot over the composer while dictation listens.
    private var listeningRow: some View {
        HStack(spacing: 8) {
            PulseDot()
            Text("Listening...").font(.body(12, .medium)).foregroundStyle(Mint.muted)
        }
        .padding(.horizontal, 20).padding(.vertical, 2)
    }

    /// Messages queued while the reply runs; each sends as its own turn once the one before
    /// finishes. Tapping the cross puts one back in the composer to edit.
    private var queuedList: some View {
        let queued = model.chat.queued
        return VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 8) {
                Image(systemName: "clock").font(.system(size: 13)).foregroundStyle(Mint.muted)
                Text(!model.chat.busy ? "Not sent"
                     : queued.count > 1 ? "Send one by one when Beep finishes" : "Sends when Beep finishes")
                    .font(.body(12, .semibold)).foregroundStyle(Mint.muted)
                Spacer(minLength: 0)
                if !model.chat.busy {
                    Button { model.sendQueued() } label: {
                        Image(systemName: "paperplane").font(.system(size: 15)).foregroundStyle(Mint.accent)
                            .frame(width: 36, height: 36).contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Send queued messages")
                }
            }
            .frame(minHeight: 32)
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(Array(queued.enumerated()), id: \.offset) { index, message in
                        HStack(spacing: 8) {
                            Text("\(index + 1)").font(.body(12, .semibold)).foregroundStyle(Mint.muted)
                                .frame(width: 16)
                            Text(!message.text.isEmpty ? message.text
                                 : message.files.first?.name ?? "\(message.images.count) image\(message.images.count > 1 ? "s" : "")")
                                .font(.body(14)).foregroundStyle(Mint.ink).lineLimit(1)
                            Spacer(minLength: 0)
                            Button {
                                guard let back = model.unqueue(index) else { return }
                                draft = draft.isEmpty ? back.text : back.text + "\n" + draft
                                photos = Array((back.images + photos).prefix(ChatPhotos.limit))
                                files = back.files + files
                                typing = true
                            } label: {
                                Image(systemName: "xmark").font(.system(size: 12, weight: .semibold))
                                    .foregroundStyle(Mint.muted)
                                    .frame(width: 36, height: 36).contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .accessibilityLabel("Edit queued message \(index + 1)")
                        }
                        .frame(minHeight: 36)
                    }
                }
            }
            .frame(maxHeight: 116)
            .fixedSize(horizontal: false, vertical: true)
        }
        .padding(.leading, 12).padding(.trailing, 4).padding(.vertical, 2)
        .chunky(radius: 12)
        .padding(.horizontal, 14).padding(.top, 6)
        .animation(.bouncy, value: queued)
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

    /// The files waiting to go with the next message, each with a remove button.
    private var fileChips: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(Array(files.enumerated()), id: \.offset) { index, file in
                    ZStack(alignment: .topTrailing) {
                        AttachedFileChip(file: file)
                        Button { files.remove(at: index) } label: {
                            Image(systemName: "xmark").font(.system(size: 9, weight: .bold)).foregroundStyle(Mint.page)
                                .frame(width: 20, height: 20).background(Circle().fill(Mint.ink))
                        }
                        .buttonStyle(.plain)
                        .padding(3)
                        .accessibilityLabel("Remove \(file.name)")
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

    /// Empties the composer after a send, in a new text field: while the keyboard has a
    /// suggestion pending, iOS keeps its own copy of the text and can put it back on a later
    /// redraw, such as when the reply starts. The keyboard stays up on the new field.
    private func clearDraft() {
        let focused = typing
        draft = ""
        field += 1
        if focused { Task { @MainActor in typing = true } }
    }
}

private struct ErrorRow<Extra: View>: View {
    let text: String
    let retry: () -> Void
    /// Shown before Retry, such as an admin's Rerun.
    @ViewBuilder var extra: Extra

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: "exclamationmark.circle").foregroundStyle(Mint.error)
            Text(text).font(.body(13)).foregroundStyle(Mint.ink)
            Spacer(minLength: 0)
            extra
            Button("Retry", action: retry).buttonStyle(TextAction(color: Mint.error))
        }
        .padding(.leading, 12).padding(.trailing, 4).padding(.vertical, 4)
        .background(RoundedRectangle(cornerRadius: 12).fill(Mint.error.opacity(0.1)))
    }
}

private struct EmptyChat: View {
    @Environment(AppModel.self) private var model
    @Environment(\.scenePhase) private var scenePhase
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
            // Updates from the user's apps sit on their own card, never inside a reply. Tapping one
            // fills the composer with its summary, for the user to send or change.
            if !model.updates.isEmpty {
                UpdatesCard(updates: model.updates, open: { update in
                    model.dismissUpdates([update.id])
                    fill(update.summary)
                }, dismiss: model.dismissUpdates)
                .padding(.top, 16)
            }
            (Text("A little help.\n".display) + Text("Right here.".display).foregroundColor(Mint.ok))
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
        .task { await model.loadUpdates() }
        .onChange(of: scenePhase) { _, phase in
            if phase == .active { Task { await model.loadUpdates() } }
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

/// The label of the scheduled turn that hands background results back to its chat, as the server names it.
let backgroundWorkLabel = "Background work"

/// Mirrors the server's memory threshold as Android shows it: above it the learner reads the turn.
private let memoryThreshold = 0.8

struct MessageView: View {
    @Environment(AppModel.self) private var model
    let message: Message
    let live: Bool
    /// The newest reply in the chat, which shows the suggested replies.
    var latest = false
    var quote: (String) -> Void = { _ in }
    @State private var showThoughts = false
    @State private var showAllTools = false
    @State private var copied = false

    var body: some View {
        switch message.role {
        case .user:
            VStack(alignment: .trailing, spacing: 0) {
                // A tap opens the menu (copy, quote, share, delete), as on Android.
                Menu {
                    promptMenu
                } label: {
                    userBubble.userBubble(bubbleShape(user: true))
                }
                .buttonStyle(.plain)
                .disabled(live)
                if !message.model.isEmpty {
                    Text(rerunCaption).font(.body(11)).foregroundStyle(Mint.muted)
                        .padding(.top, 4).padding(.trailing, 4)
                }
                // The model and thinking Jev picked under Auto. Everyone but an admin is on
                // Auto for thinking only, so they see just the level.
                if let auto = message.auto {
                    Text(autoCaption(auto)).font(.body(11)).foregroundStyle(Mint.dim)
                        .padding(.top, 4).padding(.trailing, 4)
                }
            }
            .padding(.leading, 48)
            .frame(maxWidth: .infinity, alignment: .trailing)
        case .scheduled:
            HStack(spacing: 6) {
                // The turn that hands back what helper agents and background jobs found.
                if message.text == backgroundWorkLabel {
                    Image(systemName: "arrow.uturn.backward").font(.system(size: 11))
                    Text("Background work came back").lineLimit(1)
                } else {
                    Image(systemName: "clock").font(.system(size: 11))
                    Text(message.text.isEmpty ? "Scheduled" : "Scheduled · \(message.text)").lineLimit(1)
                }
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
                    Typewriter(text: message.text, streaming: live) { MarkdownText(text: $0) }.botBubble()
                }
                // The web pages the turn used, once the reply is done.
                if !live, !message.text.isEmpty, !message.sources.isEmpty {
                    SourceChips(sources: message.sources) { model.openBrowser($0) }.padding(.top, 8)
                }
                // A finished tool with something to show is a card, once the reply is done.
                let cards = message.tools.compactMap(\.card).filter { if case .replies = $0 { false } else { true } }
                if !live, !cards.isEmpty { ToolCards(cards: cards) { model.openBrowser($0) }.padding(.top, 10) }
                if !live, !message.text.isEmpty {
                    let shown = Set(cards.compactMap { if case .link(let url) = $0 { url } else { nil } })
                    let links = linksIn(message.text).filter { !shown.contains($0) }
                    if !links.isEmpty {
                        VStack(spacing: 8) { ForEach(links, id: \.self) { LinkCard(url: $0) { model.openBrowser($0) } } }
                            .padding(.top, 8).padding(.trailing, 4)
                    }
                }
                let steps = TaskStep.latest(in: message.tools)
                if !steps.isEmpty { TaskProgressCard(steps: steps).padding(.top, 8) }
                if !message.text.isEmpty, !live { actions.transition(.opacity) }
                if latest, !model.chat.busy, let replies = message.tools.lazy.compactMap({ call -> ToolCard.Replies? in
                    if case .replies(let replies) = call.card { replies } else { nil }
                }).first {
                    SuggestedReplies(replies: replies) { model.send($0) }.padding(.top, 4)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .animation(.easeOut(duration: 0.2), value: live)
            .animation(.bouncy, value: message.approvals.count)
        }
    }

    @ViewBuilder private var promptMenu: some View {
        Button { UIPasteboard.general.string = message.text } label: { Label("Copy", systemImage: "doc.on.doc") }
        Button { quote(message.text.trimmingCharacters(in: .whitespacesAndNewlines)) } label: {
            Label("Quote in reply", systemImage: "quote.opening")
        }
        ShareLink(item: message.text) { Label("Share", systemImage: "square.and.arrow.up") }
        // Takes the reply and its tool calls with it.
        if !model.chat.busy {
            Button(role: .destructive) {
                Task { await model.delete(message) }
            } label: { Label("Delete", systemImage: "trash") }
        }
    }

    /// The text, with any pictures above it in a 220 point wide block as in the mock.
    @ViewBuilder private var userBubble: some View {
        let text = Text(message.text).font(.body(15)).foregroundStyle(Mint.onUser).lineSpacing(4).textSelection(.enabled)
        if !message.files.isEmpty {
            VStack(alignment: .leading, spacing: 6) {
                if !message.pictures.isEmpty { PictureGrid(pictures: message.pictures) }
                ForEach(Array(message.files.enumerated()), id: \.offset) { _, file in FileCard(file: file) }
                if !message.text.isEmpty {
                    text.padding(.horizontal, 10).padding(.top, 2).padding(.bottom, 6)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            .padding(4)
            .frame(width: 260)
        } else if message.pictures.isEmpty {
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

    /// Under a prompt an admin reran: the model, and the thinking when one was picked.
    private func autoCaption(_ pick: AutoPick) -> String {
        let thinking = "\(pick.thinking)"
        guard let choices = model.models?.choices, !choices.isEmpty else { return thinking }
        let name = choices.first { $0.id == pick.model }?.name ?? pick.model.split(separator: "/").last.map(String.init) ?? pick.model
        return "\(name) · \(thinking)"
    }

    private var rerunCaption: String {
        let name = model.models?.choices.first { $0.id == message.model }?.name ?? message.model
        return "Rerun on \(name)" + (message.modelThinking.map { " · \($0)" } ?? "")
    }

    /// The model this reply ran on. The server names it for turns it recorded; a reply from
    /// this session ran on its prompt's model, or the user's own when it had none.
    private func usedModel(_ models: Models) -> String? {
        if !message.model.isEmpty { return message.model }
        let messages = model.chat.messages
        guard let at = messages.firstIndex(where: { $0.id == message.id }),
            let prompt = messages[..<at].last(where: { $0.role == .user })
        else { return nil }
        if !prompt.model.isEmpty { return prompt.model }
        return message.id.hasPrefix("history-") ? nil : models.selected
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
        let calls = message.tools.filter { $0.card == nil && !quietTools.contains($0.name) }
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
            // Admins get a rerun menu under each reply; the list is empty for everyone else.
            if let models = model.models, !models.choices.isEmpty, !model.chat.busy {
                RerunButton(models: models, used: usedModel(models)) { choice, thinking in
                    Task { await model.rerun(message, model: choice, thinking: thinking) }
                }
            }
            // The specialized agent that answered, for admins next to the memory score.
            if model.models?.choices.isEmpty == false, !message.agent.isEmpty {
                Text(message.agent).font(.body(11)).foregroundStyle(Mint.dim).padding(.leading, 8)
            }
            // Jev's estimate that this turn held something worth remembering.
            if let score = message.memoryScore {
                let saved = message.tools.contains { wroteMemory($0.name, output: $0.output) }
                let note = score >= memoryThreshold && !saved ? " · nothing new" : ""
                Text("\(Int((score * 100).rounded()))%\(note)").font(.body(11)).foregroundStyle(Mint.dim)
                    .padding(.leading, 8)
            }
        }
        .buttonStyle(TextAction(color: Mint.muted))
        .padding(.leading, -8)
        .padding(.top, 2)
    }
}

/// Admin only: runs the prompt behind a reply again on a model picked from the menu. Holding a
/// model offers a thinking level for that one rerun; `pick` gets nil for a tap.
struct RerunButton: View {
    let models: Models
    let used: String?
    var color = Mint.muted
    var prominent = false
    let pick: (String, ReasoningLevel?) -> Void
    @State private var open = false
    @State private var held: String?

    var body: some View {
        Button { held = nil; open = true } label: {
            HStack(spacing: prominent ? 2 : 6) {
                if prominent {
                    Text("Rerun").font(.body(14, .semibold))
                } else {
                    Image(systemName: "arrow.clockwise").font(.system(size: 12))
                    Text("Rerun").font(.body(11))
                }
                Image(systemName: "chevron.down").font(.system(size: prominent ? 11 : 9, weight: .semibold))
            }
        }
        .buttonStyle(TextAction(color: color))
        .accessibilityHint("Pick a model to run the prompt again on")
        .popover(isPresented: $open) {
            menu.presentationCompactAdaptation(.popover).presentationBackground(Mint.surface)
        }
        .closesForLive { open = false }
    }

    private var menu: some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(models.choices, id: \.id) { choice in
                row(choice)
                if held == choice.id { thinking(choice).transition(.opacity) }
            }
        }
        .padding(.vertical, 6)
        .frame(width: 290)
        .sensoryFeedback(.impact, trigger: held) { _, now in now != nil }
    }

    private func row(_ choice: ModelChoice) -> some View {
        let isUsed = choice.id == used
        let note = isUsed ? "Used for this reply"
            : choice.id == models.selected ? "Your model"
            : choice.default ? "Default" : choice.id
        return HStack(spacing: 8) {
            VStack(alignment: .leading, spacing: 1) {
                Text(choice.name).font(.body(14, isUsed ? .semibold : .regular))
                    .foregroundStyle(isUsed ? Mint.brand : Mint.ink)
                Text(note).font(.body(11)).foregroundStyle(isUsed ? Mint.brand : Mint.muted)
                    .lineLimit(1).truncationMode(.middle)
            }
            Spacer(minLength: 0)
            if isUsed {
                Image(systemName: "checkmark").font(.system(size: 13, weight: .semibold)).foregroundStyle(Mint.brand)
            }
        }
        .padding(.horizontal, 12).padding(.vertical, 6)
        .frame(minHeight: 48)
        .background(isUsed ? Mint.brand.opacity(0.12) : .clear)
        .contentShape(Rectangle())
        .onTapGesture { open = false; pick(choice.id, nil) }
        .onLongPressGesture { withAnimation(.snappy) { held = held == choice.id ? nil : choice.id } }
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.isButton)
        .accessibilityLabel("Rerun on \(choice.name)")
        .accessibilityAction(named: "Pick thinking") { held = choice.id }
    }

    private func thinking(_ choice: ModelChoice) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("Thinking for this rerun").font(.body(11)).foregroundStyle(Mint.muted)
            HStack(spacing: 4) {
                ForEach([ReasoningLevel.off, .low, .medium, .high], id: \.self) { level in
                    FilterChip(label: "\(level)".capitalized, selected: false) { open = false; pick(choice.id, level) }
                }
            }
        }
        .padding(.horizontal, 12).padding(.bottom, 8)
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

    /// A card asking for a routine's grant. Allow already saves the grant, which ends on its
    /// own, so Always has nothing more to give and is left off.
    private var forRoutine: Bool { prompt.approval.action.hasSuffix("for a routine") }

    var body: some View {
        if prompt.withdrawn {
            Label("\(prompt.approval.action) with \(prompt.approval.app): no longer waiting", systemImage: "shield.slash")
                .font(.body(12, .medium)).foregroundStyle(Mint.muted)
        } else if prompt.skipped {
            Label("\(prompt.approval.action) with \(prompt.approval.app): not answered, you wrote again, so not done",
                  systemImage: "shield.slash")
                .font(.body(12, .medium)).foregroundStyle(Mint.muted)
        } else if let answer = prompt.answer {
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
                        Text(forRoutine ? "Each run does this without asking until it ends"
                             : "\(prompt.approval.app) asks before it does this")
                            .font(.body(12)).foregroundStyle(Mint.muted)
                    }
                }
                .padding(.horizontal, 14).padding(.vertical, 12)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Mint.tints[2])
                Rectangle().fill(Mint.ledge).frame(height: 1.5)
                let split = ApprovalRecords(prompt.approval.fields)
                if !split.fields.isEmpty {
                    Grid(alignment: .leading, horizontalSpacing: 8, verticalSpacing: 4) {
                        ForEach(Array(split.fields.enumerated()), id: \.offset) { _, field in
                            GridRow {
                                Text(field.label).foregroundStyle(Mint.muted)
                                Text(field.value).lineLimit(6)
                            }
                        }
                    }
                    .font(.body(13))
                    .padding(.horizontal, 14).padding(.top, 12)
                }
                if !split.records.isEmpty {
                    RecordStrip(records: split.records)
                }
                HStack(spacing: 8) {
                    Button(split.records.isEmpty ? "Allow" : "Allow all \(split.records.count)") {
                        model.answer(prompt, .allow)
                    }
                        .font(.body(14, .semibold)).foregroundStyle(.white)
                        .padding(.horizontal, 20).frame(minHeight: 44)
                        .chunky(Mint.ok, radius: 12)
                    if !forRoutine {
                        Button("Always") { model.answer(prompt, .always) }.buttonStyle(PillButton())
                    }
                    Spacer()
                    Button("Deny") { model.answer(prompt, .deny) }
                        .font(.body(14, .medium)).foregroundStyle(Mint.error)
                }
                .padding(14)
            }
            .foregroundStyle(Mint.ink)
            .card()
        }
    }

    private func answerText(_ answer: ApprovalDecision) -> String {
        switch answer {
        case .allow: forRoutine ? "Allowed for the routine" : "Allowed once"
        case .always: "Always allowed for \(prompt.approval.app)"
        case .deny: "Denied"
        }
    }
}

/// A card's fields split into the call's own and, when it names several records, one group
/// per record. Each record repeats the same labels ("Subject", "From", "Subject", ...), so the
/// first label seen twice starts every record.
struct ApprovalRecords {
    let fields: [ApprovalField]
    let records: [[ApprovalField]]

    init(_ all: [ApprovalField]) {
        var seen = Set<String>()
        guard let repeated = all.first(where: { !seen.insert($0.label).inserted })?.label,
              let start = all.firstIndex(where: { $0.label == repeated }) else {
            fields = all
            records = []
            return
        }
        var groups: [[ApprovalField]] = []
        for field in all[start...] {
            if field.label == repeated || groups.isEmpty { groups.append([]) }
            groups[groups.count - 1].append(field)
        }
        fields = Array(all[..<start])
        records = groups
    }
}

/// The records a call acts on, one small card each, swiped through sideways.
private struct RecordStrip: View {
    let records: [[ApprovalField]]

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(alignment: .top, spacing: 8) {
                    ForEach(Array(records.enumerated()), id: \.offset) { _, record in
                        VStack(alignment: .leading, spacing: 2) {
                            if let first = record.first {
                                Text(first.value).font(.body(13, .medium)).lineLimit(2)
                            }
                            ForEach(Array(record.dropFirst().enumerated()), id: \.offset) { _, field in
                                Text(field.value).font(.body(12)).foregroundStyle(Mint.muted).lineLimit(1)
                            }
                        }
                        .padding(.horizontal, 12).padding(.vertical, 10)
                        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                        .background(RoundedRectangle(cornerRadius: 12).fill(Mint.page))
                        .containerRelativeFrame(.horizontal) { width, _ in width * 0.78 }
                    }
                }
                .scrollTargetLayout()
                .padding(.horizontal, 14)
            }
            .scrollTargetBehavior(.viewAligned)
            .blocksChatSwipe()
            Text("Swipe to see all \(records.count)")
                .font(.body(12)).foregroundStyle(Mint.muted)
                .padding(.horizontal, 14)
        }
        .padding(.top, 12)
    }
}

/// Questions a helper agent, a background job or a coding agent on a machine asked while no turn
/// ran. They come as notices, so they sit at the end of the chat under a caption of their own.
private struct BackgroundQuestions: View {
    let prompts: [ApprovalPrompt]

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 6) {
                Image(systemName: "square.stack.3d.up").font(.system(size: 11))
                Text(prompts.contains { $0.answer == nil && !$0.withdrawn }
                     ? "Background work is waiting for you" : "Background work").lineLimit(1)
            }
            .font(.body(11)).foregroundStyle(Mint.dim)
            .frame(maxWidth: .infinity)
            ForEach(prompts, id: \.queryId) { ApprovalCard(prompt: $0) }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .animation(.bouncy, value: prompts)
    }
}

/// Replies as markdown: paragraphs, bullets, headings, code and tables, with inline styling.
struct MarkdownText: View {
    let text: String
    var size: CGFloat = 15
    var color: Color = Mint.ink

    var body: some View {
        // As on Android: about one and a half lines per line, and a gap between paragraphs.
        VStack(alignment: .leading, spacing: size >= 15 ? 10 : 6) {
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
                case .picture(let picture):
                    ReplyImage(picture: picture).padding(.vertical, 2)
                case .gallery(let pictures):
                    ReplyGallery(pictures: pictures).padding(.vertical, 2)
                case .paragraph(let line):
                    inline(line)
                }
            }
        }
        .font(.body(size))
        .foregroundStyle(color)
        .lineSpacing(size * 0.4)
        .textSelection(.enabled)
    }

    private enum Block {
        case paragraph(String), bullet(String), heading(String), code(String), table(Table), rule
        case products([Product])
        case picture(ReplyPicture), gallery([ReplyPicture])
    }

    private static let imageToken = try? NSRegularExpression(
        pattern: #"!\[([^\]]*)]\((https://[^\s)]+|data:image/[^\s)]+)\)"#)

    /// A line split around its images: text before an image, the image, then the rest.
    private static func pictures(in line: String) -> [Block]? {
        guard let regex = imageToken else { return nil }
        let matches = regex.matches(in: line, range: NSRange(line.startIndex..., in: line))
        guard !matches.isEmpty else { return nil }
        var out: [Block] = []
        var rest = line.startIndex
        for match in matches {
            guard let whole = Range(match.range, in: line), let alt = Range(match.range(at: 1), in: line),
                  let url = Range(match.range(at: 2), in: line) else { continue }
            let before = line[rest..<whole.lowerBound].trimmingCharacters(in: .whitespaces)
            if !before.isEmpty { out.append(.paragraph(before)) }
            out.append(.picture(ReplyPicture(url: String(line[url]), alt: String(line[alt]))))
            rest = whole.upperBound
        }
        let after = line[rest...].trimmingCharacters(in: .whitespaces)
        if !after.isEmpty { out.append(.paragraph(after)) }
        return out
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
        .blocksChatSwipe()
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
            } else if let parts = Self.pictures(in: line) {
                for part in parts {
                    // Images with nothing but blank lines between them show as one row.
                    if case .picture(let picture) = part {
                        if case .picture(let previous) = out.last {
                            out[out.count - 1] = .gallery([previous, picture])
                            continue
                        } else if case .gallery(let row) = out.last {
                            out[out.count - 1] = .gallery(row + [picture])
                            continue
                        }
                    }
                    out.append(part)
                }
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
        var styled = (try? AttributedString(markdown: line, options: options)) ?? AttributedString(line)
        linkBareAddresses(&styled)
        return Text(styled)
    }

    /// Makes bare web addresses tappable, as Android and the Mac do. Markdown links
    /// keep their own target.
    private func linkBareAddresses(_ text: inout AttributedString) {
        let plain = String(text.characters)
        guard plain.contains("http"), let detector = try? NSDataDetector(types: NSTextCheckingResult.CheckingType.link.rawValue) else { return }
        for match in detector.matches(in: plain, range: NSRange(plain.startIndex..., in: plain)) {
            guard let url = match.url, ["http", "https"].contains(url.scheme?.lowercased()),
                  let found = Range(match.range, in: plain),
                  let lower = AttributedString.Index(found.lowerBound, within: text),
                  let upper = AttributedString.Index(found.upperBound, within: text),
                  !text[lower..<upper].runs.contains(where: { $0.link != nil }) else { continue }
            text[lower..<upper].link = url
        }
    }
}

extension View {
    /// Opens at the latest message instead of scrolling down from the top. A chat shorter
    /// than the screen still starts at the top, as on Android, rather than floating above
    /// the composer.
    @ViewBuilder func chatScrollAnchor() -> some View {
        if #available(iOS 18, *) {
            defaultScrollAnchor(.bottom, for: .initialOffset)
                .defaultScrollAnchor(.bottom, for: .sizeChanges)
        } else {
            defaultScrollAnchor(.bottom)
        }
    }
}

/// A dot fading in and out, while dictation listens.
private struct PulseDot: View {
    @State private var bright = false

    var body: some View {
        Circle().fill(Mint.accent).frame(width: 7, height: 7)
            .opacity(bright ? 1 : 0.4)
            .onAppear { withAnimation(.easeInOut(duration: 0.6).repeatForever(autoreverses: true)) { bright = true } }
    }
}

/// A ring spreading out from the mic button while it listens.
private struct MicRing: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var spread = false

    var body: some View {
        Circle().fill(Mint.accent.opacity(spread ? 0 : 0.45))
            .scaleEffect(spread ? 1.7 : 1)
            .onAppear {
                guard !reduceMotion else { return }
                withAnimation(.easeOut(duration: 1.1).repeatForever(autoreverses: false)) { spread = true }
            }
    }
}
