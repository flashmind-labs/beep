import AuthenticationServices
import Network
import SwiftUI

/// The apps Beep can work in, as on Android. Composio apps and added MCP servers sign in on a
/// web page in the app, and the list refreshes when it closes.
struct AppsView: View {
    @Environment(AppModel.self) private var model
    let back: () -> Void
    @State private var apps: [AppIntegration] = []
    @State private var loading = true
    @State private var error = ""
    @State private var busy: String?
    /// Nil until the server answers; a server too old to know them never does, and the section stays hidden.
    @State private var servers: [McpServer]?
    @State private var open: AppIntegration?
    @State private var openServer: McpServer?
    @State private var adding = false

    var body: some View {
        VStack(spacing: 0) {
            BackBar(title: "Apps", back: back)
            ScrollView {
                VStack(alignment: .leading, spacing: 10) {
                    Text("Connect an app once, then just ask. \"What's on my calendar?\" or \"Add this to Notion\" work from any chat.")
                        .font(.body(14)).foregroundStyle(Mint.muted).lineSpacing(4).padding(.bottom, 6)
                    if !error.isEmpty { Text(error).font(.body(13)).foregroundStyle(Mint.error) }
                    if !apps.isEmpty, !apps.contains(where: { $0.kind == .composio && ($0.configured || $0.connected) }) {
                        Text("Connecting apps needs a Composio API key on the server.")
                            .font(.body(13)).foregroundStyle(Color(hex: 0xB7791F))
                    }
                    if loading {
                        ProgressView().tint(Mint.ok).frame(maxWidth: .infinity).padding(40)
                    }
                    let connected = apps.filter(\.connected)
                    let available = apps.filter { !$0.connected }
                    if !connected.isEmpty {
                        SectionLabel(text: "Connected")
                        ForEach(connected, id: \.slug) { appRow($0) }
                    }
                    if !available.isEmpty {
                        SectionLabel(text: connected.isEmpty ? "Available" : "More apps")
                        ForEach(available, id: \.slug) { appRow($0) }
                    }
                    if !loading, let servers {
                        SectionLabel(text: "Your servers")
                        if servers.isEmpty {
                            Text("Add any remote MCP server by its address. Beep asks before every action it takes there.")
                                .font(.body(13)).foregroundStyle(Mint.muted).lineSpacing(3)
                        }
                        ForEach(servers, id: \.id) { serverRow($0) }
                        Button { adding = true } label: {
                            Text("+ Add MCP server").font(.body(14, .semibold)).foregroundStyle(Mint.ink)
                                .padding(.horizontal, 16).frame(minHeight: 40)
                        }
                        .buttonStyle(SinkButton())
                    }
                }
                .padding(.horizontal, 18).padding(.vertical, 8)
            }
        }
        .background(Mint.page)
        .navigationDestination(item: $open) { app in
            AppPermissionsView(app: app, back: { open = nil }) {
                open = nil
                disconnect(app)
            }
            .toolbar(.hidden, for: .navigationBar)
        }
        .navigationDestination(item: $openServer) { server in
            McpServerView(server: server, busy: busy == server.id, error: error, back: { openServer = nil },
                          signIn: { signIn(server) }) {
                openServer = nil
                remove(server)
            }
            .toolbar(.hidden, for: .navigationBar)
        }
        .sheet(isPresented: $adding) {
            AddMcpServerSheet { result in
                adding = false
                Task { await connected(result) }
            }
        }
        .task {
            while !model.connected { try? await Task.sleep(for: .milliseconds(200)) }
            await refresh()
        }
    }

    private func appRow(_ app: AppIntegration) -> some View {
        Button { if app.connected { open = app } } label: {
            HStack(spacing: 14) {
                AppLogo(slug: app.slug, name: app.name, logoUrl: app.logoUrl)
                VStack(alignment: .leading, spacing: 2) {
                    Text(app.name).font(.body(15, .semibold)).foregroundStyle(Mint.ink).lineLimit(1)
                    Text(app.description).font(.body(12)).foregroundStyle(Mint.muted).lineLimit(2)
                        .multilineTextAlignment(.leading)
                }
                Spacer(minLength: 10)
                if busy == app.slug {
                    ProgressView().tint(Mint.ok).padding(.horizontal, 18)
                } else if app.connected {
                    ConnectedChip()
                } else if !app.configured {
                    Text("Unavailable").font(.body(12)).foregroundStyle(Mint.dim).frame(width: 76, alignment: .leading)
                } else {
                    Button("Connect") { connect(app) }.buttonStyle(SmallAccentButton())
                }
            }
            .padding(14)
            .frame(maxWidth: .infinity, alignment: .leading)
            .card()
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    private func serverRow(_ server: McpServer) -> some View {
        Button { openServer = server } label: {
            HStack(spacing: 14) {
                AppLogo(slug: server.id, name: server.name)
                VStack(alignment: .leading, spacing: 2) {
                    Text(server.name).font(.body(15, .semibold)).foregroundStyle(Mint.ink).lineLimit(1)
                    Text(URL(string: server.url)?.host() ?? server.url).font(.body(12)).foregroundStyle(Mint.muted).lineLimit(1)
                }
                Spacer(minLength: 10)
                if busy == server.id {
                    ProgressView().tint(Mint.ok).padding(.horizontal, 18)
                } else if server.connected {
                    HStack(spacing: 2) {
                        Text(server.tools.count == 1 ? "1 tool" : "\(server.tools.count) tools")
                            .font(.body(12, .semibold)).foregroundStyle(Mint.ink)
                        Image(systemName: "chevron.right").font(.system(size: 12, weight: .medium)).foregroundStyle(Mint.muted)
                    }
                } else {
                    Button(server.signIn ? "Sign in" : "Retry") { signIn(server) }.buttonStyle(SmallAccentButton())
                }
            }
            .padding(14)
            .frame(maxWidth: .infinity, alignment: .leading)
            .card()
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    private func refresh() async {
        do {
            apps = try await model.apps()
            servers = await model.mcpServers() ?? servers
            if let shown = openServer { openServer = servers?.first { $0.id == shown.id } }
            error = ""
        } catch {
            self.error = describe(error)
        }
        loading = false
        busy = nil
    }

    private func connect(_ app: AppIntegration) {
        busy = app.slug
        Task {
            do {
                if app.kind == .fastmail {
                    try await LoopbackAuth.run(start: { try await model.startAppAuth(app.slug, redirect: $0) }) {
                        try await model.finishAppAuth(app.slug, code: $0.code, state: $0.state, issuer: $0.issuer)
                    }
                } else {
                    _ = try? await WebSignIn.open(URL(string: try await model.connectApp(app.slug)))
                }
            } catch {
                self.error = describe(error)
            }
            await refresh()
        }
    }

    private func disconnect(_ app: AppIntegration) {
        busy = app.slug
        Task {
            do { try await model.disconnectApp(app.slug) } catch { self.error = describe(error) }
            await refresh()
        }
    }

    /// Opens the server's sign-in when it needs one; the link back finishes it.
    private func connected(_ result: McpConnect) async {
        if let auth = result.authUrl {
            do {
                if let back = try await WebSignIn.open(URL(string: auth)), let callback = mcpCallback(url: back.absoluteString) {
                    _ = try await model.finishMcpAuth(callback)
                }
            } catch let failure as BeepError {
                error = describe(failure)
            } catch {
                // The user closed the sign-in page.
            }
        }
        await refresh()
    }

    private func signIn(_ server: McpServer) {
        busy = server.id
        Task {
            do { await connected(try await model.connectMcpServer(server.id)) } catch {
                self.error = describe(error)
                busy = nil
            }
        }
    }

    private func remove(_ server: McpServer) {
        busy = server.id
        Task {
            do { try await model.removeMcpServer(server.id) } catch { self.error = describe(error) }
            await refresh()
        }
    }
}

extension AppIntegration: Identifiable {
    public var id: String { slug }
}

extension McpServer: Identifiable {}

private struct SectionLabel: View {
    let text: String

    var body: some View {
        Text(text.uppercased()).font(.body(11, .semibold)).tracking(1.2).foregroundStyle(Mint.dim)
            .padding(.top, 10).padding(.bottom, 2)
    }
}

/// A connected app's state; tapping the row opens its permissions.
private struct ConnectedChip: View {
    var body: some View {
        HStack(spacing: 4) {
            Image(systemName: "checkmark").font(.system(size: 11, weight: .bold)).foregroundStyle(Mint.ok)
            Text("Connected").font(.body(12, .semibold)).foregroundStyle(Mint.ink)
            Image(systemName: "chevron.right").font(.system(size: 11, weight: .medium)).foregroundStyle(Mint.muted)
        }
        .padding(.leading, 10).padding(.trailing, 8).padding(.vertical, 6)
        .background(Capsule().fill(Mint.tints[1]))
    }
}

/// The small coral Connect button.
private struct SmallAccentButton: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        let shape = RoundedRectangle(cornerRadius: 12)
        let down: CGFloat = configuration.isPressed ? 3 : 0
        configuration.label
            .font(.body(13, .semibold)).foregroundStyle(Mint.ink)
            .padding(.horizontal, 14).frame(minHeight: 34)
            .background(shape.fill(Mint.accent))
            .overlay(shape.stroke(Mint.ledge, lineWidth: 2))
            .offset(x: down, y: down)
            .animation(.sink, value: configuration.isPressed)
            .background(shape.fill(Mint.ledge).offset(x: 3, y: 3))
    }
}

/// Any label on a white chunky key that sinks while pressed.
private struct SinkButton: ButtonStyle {
    var fill: Color = Mint.surface

    func makeBody(configuration: Configuration) -> some View {
        let shape = RoundedRectangle(cornerRadius: 12)
        let down: CGFloat = configuration.isPressed ? 3 : 0
        configuration.label
            .background(shape.fill(fill))
            .overlay(shape.stroke(Mint.ledge, lineWidth: 2))
            .offset(x: down, y: down)
            .animation(.sink, value: configuration.isPressed)
            .background(shape.fill(Mint.ledge).offset(x: 3, y: 3))
    }
}

/// The app's bundled logo, else the server's logo, else its initial on a tint. Logos sit on
/// white so dark marks like GitHub's stay visible.
struct AppLogo: View {
    let slug: String
    let name: String
    var logoUrl: String?
    var size: CGFloat = 42

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 12)
        SwiftUI.Group {
            if UIImage(named: "logo_" + slug) != nil {
                Image("logo_" + slug).resizable().scaledToFit().frame(width: size * 0.6, height: size * 0.6)
                    .frame(width: size, height: size).background(shape.fill(.white))
            } else if let url = logoUrl.flatMap(URL.init(string:)) {
                AsyncImage(url: url) { phase in
                    if let image = phase.image {
                        image.resizable().scaledToFit().frame(width: size * 0.7, height: size * 0.7)
                            .frame(width: size, height: size).background(shape.fill(.white))
                    } else {
                        initial
                    }
                }
            } else {
                initial
            }
        }
        .clipShape(shape)
    }

    private var initial: some View {
        Text(name.prefix(1).uppercased()).font(.body(size * 0.43, .bold)).foregroundStyle(Mint.ink)
            .frame(width: size, height: size)
            .background(RoundedRectangle(cornerRadius: 12).fill(Mint.tint(for: name)))
    }
}

private let levels: [(PermissionLevel, String, String)] = [
    (.ask, "Ask first", "Beep shows what it is about to do and waits for you."),
    (.allow, "Always allow", "Beep sends, changes and deletes without asking."),
    (.readOnly, "Read only", "Beep can search and read, never change anything."),
]

/// How much Beep may do in one connected app: a level, and under Ask first, actions that never ask.
private struct AppPermissionsView: View {
    @Environment(AppModel.self) private var model
    let app: AppIntegration
    let back: () -> Void
    let disconnect: () -> Void
    @State private var permissions: AppPermissions?
    @State private var error = ""

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 12) {
                Button(action: back) { Image(systemName: "arrow.left") }.buttonStyle(ChunkyButton())
                    .accessibilityLabel("Back")
                AppLogo(slug: app.slug, name: app.name, logoUrl: app.logoUrl, size: 32)
                Text(app.name).font(.body(20, .semibold)).foregroundStyle(Mint.ink).lineLimit(1)
                Spacer()
            }
            .padding(.horizontal, 18).padding(.top, 6).padding(.bottom, 10)
            ScrollView {
                VStack(alignment: .leading, spacing: 10) {
                    if !error.isEmpty { Text(error).font(.body(13)).foregroundStyle(Mint.error) }
                    if let current = permissions {
                        SectionLabel(text: "When Beep acts in \(app.name)")
                        ForEach(levels, id: \.1) { level, title, detail in
                            levelRow(current, level, title, detail)
                        }
                        if !current.actions.isEmpty {
                            SectionLabel(text: "Actions")
                            VStack(spacing: 0) {
                                ForEach(Array(current.actions.enumerated()), id: \.element.tool) { index, action in
                                    if index > 0 { Rectangle().fill(Mint.soft).frame(height: 1) }
                                    actionRow(current, action)
                                }
                            }
                            .padding(.horizontal, 14).padding(.vertical, 4)
                            .card()
                        }
                        Text("Searching and reading is always allowed.").font(.body(12)).foregroundStyle(Mint.muted)
                            .padding(.horizontal, 4)
                    } else if error.isEmpty {
                        ProgressView().tint(Mint.ok).frame(maxWidth: .infinity).padding(40)
                    }
                    Button("Disconnect \(app.name)", action: disconnect)
                        .buttonStyle(TextAction(color: Mint.error))
                        .frame(maxWidth: .infinity).padding(.top, 12)
                }
                .padding(.horizontal, 18).padding(.vertical, 8)
            }
        }
        .background(Mint.page)
        .task(id: app.slug) {
            do { permissions = try await model.appPermissions(app.slug) } catch { self.error = describe(error) }
        }
    }

    private func levelRow(_ current: AppPermissions, _ level: PermissionLevel, _ title: String, _ detail: String) -> some View {
        let selected = current.level == level
        return Button {
            if !selected { save(AppPermissions(level: level, actions: current.actions)) }
        } label: {
            HStack(spacing: 12) {
                Image(systemName: selected ? "largecircle.fill.circle" : "circle").font(.system(size: 20))
                    .foregroundStyle(selected ? Mint.ok : Mint.muted)
                VStack(alignment: .leading, spacing: 1) {
                    Text(title).font(.body(14, .semibold)).foregroundStyle(Mint.ink)
                    Text(detail).font(.body(12)).foregroundStyle(Mint.muted).multilineTextAlignment(.leading)
                }
                Spacer(minLength: 0)
            }
            .padding(14)
            .card()
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }

    private func actionRow(_ current: AppPermissions, _ action: AppAction) -> some View {
        let (tag, tint): (String, Color) = if current.level == .readOnly {
            ("Off", Mint.code)
        } else if current.level == .allow || action.allowed {
            ("Allowed", Mint.tints[1])
        } else {
            ("Asks", Mint.tints[2])
        }
        return HStack(spacing: 10) {
            Text(action.label).font(.body(14)).foregroundStyle(Mint.ink)
            Spacer(minLength: 0)
            Text(tag).font(.body(11, .semibold)).foregroundStyle(Mint.ink)
                .padding(.horizontal, 8).padding(.vertical, 3).background(Capsule().fill(tint))
            if current.level == .ask {
                Toggle("Always allow \(action.label)", isOn: Binding(get: { action.allowed }, set: { allowed in
                    save(AppPermissions(level: current.level, actions: current.actions.map {
                        $0.tool == action.tool ? AppAction(tool: $0.tool, label: $0.label, allowed: allowed) : $0
                    }))
                }))
                .labelsHidden().tint(Mint.ok)
            }
        }
        .frame(minHeight: 52)
    }

    private func save(_ next: AppPermissions) {
        let before = permissions
        permissions = next
        Task {
            do {
                try await model.setAppPermissions(app.slug, next)
                error = ""
            } catch {
                permissions = before
                self.error = describe(error)
            }
        }
    }
}

/// One added server: its address, whether it works, its tools, and Remove.
private struct McpServerView: View {
    let server: McpServer
    let busy: Bool
    let error: String
    let back: () -> Void
    let signIn: () -> Void
    let remove: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 12) {
                Button(action: back) { Image(systemName: "arrow.left") }.buttonStyle(ChunkyButton())
                    .accessibilityLabel("Back")
                AppLogo(slug: server.id, name: server.name, size: 32)
                Text(server.name).font(.body(20, .semibold)).foregroundStyle(Mint.ink).lineLimit(1)
                Spacer()
            }
            .padding(.horizontal, 18).padding(.top, 6).padding(.bottom, 10)
            ScrollView {
                VStack(alignment: .leading, spacing: 10) {
                    Text(server.url).font(.system(size: 12, design: .monospaced)).foregroundStyle(Mint.ink).lineLimit(2)
                        .padding(.horizontal, 14).padding(.vertical, 10)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .card()
                    if !error.isEmpty { Text(error).font(.body(13)).foregroundStyle(Mint.error) }
                    HStack(spacing: 8) {
                        Circle().fill(server.connected ? Mint.ok : Mint.error).frame(width: 8, height: 8)
                        Text(status).font(.body(13, .semibold)).foregroundStyle(Mint.ink)
                    }
                    if server.connected, !server.tools.isEmpty {
                        SectionLabel(text: "Tools")
                        VStack(spacing: 0) {
                            ForEach(Array(server.tools.enumerated()), id: \.offset) { index, tool in
                                if index > 0 { Rectangle().fill(Mint.soft).frame(height: 1) }
                                HStack {
                                    Text(toolLabel(tool)).font(.body(14)).foregroundStyle(Mint.ink)
                                    Spacer()
                                    Text("Asks").font(.body(11, .semibold)).foregroundStyle(Mint.ink)
                                        .padding(.horizontal, 8).padding(.vertical, 3).background(Capsule().fill(Mint.tints[2]))
                                }
                                .frame(minHeight: 44)
                            }
                        }
                        .padding(.horizontal, 14).padding(.vertical, 4)
                        .card()
                        Text("Beep asks before every tool on a server you add. Tap Always when it asks to stop asking for this server.")
                            .font(.body(12)).foregroundStyle(Mint.muted).padding(.horizontal, 4)
                    }
                    if !server.connected {
                        Text(server.signIn ? "The server stopped accepting Beep's sign-in. Sign in again to use its tools."
                             : "Beep could not reach this server. It tries again when you ask.")
                            .font(.body(13)).foregroundStyle(Mint.muted)
                        if busy {
                            ProgressView().tint(Mint.ok)
                        } else {
                            Button(server.signIn ? "Sign in again" : "Try again", action: signIn)
                                .buttonStyle(SmallAccentButton())
                        }
                    }
                    Button("Remove server", action: remove)
                        .buttonStyle(TextAction(color: Mint.error))
                        .frame(maxWidth: .infinity).padding(.top, 12)
                }
                .padding(.horizontal, 18).padding(.vertical, 8)
            }
        }
        .background(Mint.page)
    }

    private var status: String {
        if server.connected { return server.tools.count == 1 ? "Connected, 1 tool" : "Connected, \(server.tools.count) tools" }
        return server.signIn ? "Sign-in expired" : "Not connected"
    }

    /// A tool's name as the server gave it, such as `search_pages`, in words.
    private func toolLabel(_ name: String) -> String {
        let words = name.replacingOccurrences(of: "_", with: " ").replacingOccurrences(of: "-", with: " ")
            .trimmingCharacters(in: .whitespaces)
        return words.prefix(1).uppercased() + words.dropFirst()
    }
}

/// Name and URL of a new server. Adding it connects, and returns the sign-in page when it needs one.
private struct AddMcpServerSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let added: (McpConnect) -> Void
    @State private var name = ""
    @State private var url = ""
    @State private var working = false
    @State private var error = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Add MCP server").font(.display(22)).foregroundStyle(Mint.ink)
            field("Name", "Team wiki", text: $name)
            field("Server URL", "https://example.com/mcp", text: $url, mono: true)
            if working {
                HStack(spacing: 8) {
                    ProgressView().tint(Mint.ok)
                    Text("Connecting to the server…").font(.body(13)).foregroundStyle(Mint.muted)
                }
            } else if !error.isEmpty {
                Text(error).font(.body(13)).foregroundStyle(Mint.error)
            } else {
                Text("Use the server's https address. If it needs an account, Beep opens its sign-in in the browser.")
                    .font(.body(12)).foregroundStyle(Mint.muted)
            }
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.buttonStyle(TextAction()).disabled(working)
                Button(error.isEmpty ? "Add" : "Try again") { add() }
                    .buttonStyle(SmallAccentButton())
                    .disabled(working || name.trimmingCharacters(in: .whitespaces).isEmpty
                              || url.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(20)
        .frame(maxHeight: .infinity, alignment: .top)
        .background(Mint.surface)
        .presentationDetents([.height(360)])
        .interactiveDismissDisabled(working)
    }

    private func field(_ label: String, _ placeholder: String, text: Binding<String>, mono: Bool = false) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(label).font(.body(12, .semibold)).foregroundStyle(Mint.muted)
            TextField("", text: text, prompt: Text(placeholder).foregroundStyle(Mint.dim))
                .font(mono ? .system(size: 13, design: .monospaced) : .body(15))
                .foregroundStyle(Mint.ink)
                .textInputAutocapitalization(mono ? .never : .sentences)
                .autocorrectionDisabled(mono)
                .keyboardType(mono ? .URL : .default)
                .padding(.horizontal, 14).frame(minHeight: 48)
                .background(RoundedRectangle(cornerRadius: 12).fill(Mint.page))
                .overlay(RoundedRectangle(cornerRadius: 12).stroke(Mint.soft, lineWidth: 1))
                .disabled(working)
        }
    }

    private func add() {
        working = true
        error = ""
        Task {
            do {
                added(try await model.addMcpServer(name: String(name.trimmingCharacters(in: .whitespaces).prefix(60)),
                                                   url: String(url.trimmingCharacters(in: .whitespaces).prefix(2000))))
            } catch {
                self.error = describe(error)
            }
            working = false
        }
    }
}

/// A sign-in page shown in the app, which closes on a `beep://` link back.
@MainActor
enum WebSignIn {
    private static let anchor = Anchor()
    private static var session: ASWebAuthenticationSession?

    /// The link the page went back to, or nil when it was closed.
    static func open(_ url: URL?) async throws -> URL? {
        guard let url else { return nil }
        return try await withCheckedThrowingContinuation { continuation in
            let session = ASWebAuthenticationSession(url: url, callbackURLScheme: "beep") { back, error in
                if let back { continuation.resume(returning: back) } else if let error,
                    (error as? ASWebAuthenticationSessionError)?.code != .canceledLogin {
                    continuation.resume(throwing: error)
                } else {
                    continuation.resume(returning: nil)
                }
            }
            session.presentationContextProvider = anchor
            self.session = session
            session.start()
        }
    }

    /// Closes the page shown, as when the sign-in finished another way.
    static func close() {
        session?.cancel()
        session = nil
    }

    private final class Anchor: NSObject, ASWebAuthenticationPresentationContextProviding {
        func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
            MainActor.assumeIsolated {
                UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
                    .flatMap(\.windows).first(where: \.isKeyWindow) ?? ASPresentationAnchor()
            }
        }
    }
}

/// What the provider sent back to the localhost redirect.
struct AuthRedirect {
    let code: String
    let state: String
    let issuer: String?
}

/// Catches an OAuth redirect for providers that only allow `http://localhost`, such as Fastmail,
/// as Android's LoopbackAuth does. It listens on a loopback port, opens the sign-in page in the
/// app, and hands the code on once the page goes to the redirect. Tokens stay on the server.
@MainActor
enum LoopbackAuth {
    struct Failed: LocalizedError {
        let errorDescription: String?
    }

    static func run(start: (String) async throws -> String, finish: @escaping (AuthRedirect) async throws -> Void) async throws {
        let parameters = NWParameters.tcp
        parameters.requiredInterfaceType = .loopback
        let listener = try NWListener(using: parameters)
        defer { listener.cancel() }
        let caught = AsyncStream<Result<AuthRedirect, Error>>.makeStream()
        listener.newConnectionHandler = { connection in
            connection.start(queue: .main)
            receive(connection) { result in caught.continuation.yield(result) }
        }
        let port = try await ready(listener)
        let page = try await start("http://localhost:\(port)/callback")
        let closed = Task { @MainActor in
            _ = try? await WebSignIn.open(URL(string: page))
            caught.continuation.finish()
        }
        defer { closed.cancel() }
        for await result in caught.stream {
            WebSignIn.close()
            try await finish(try result.get())
            return
        }
    }

    private static func ready(_ listener: NWListener) async throws -> UInt16 {
        try await withCheckedThrowingContinuation { continuation in
            listener.stateUpdateHandler = { state in
                switch state {
                case .ready:
                    listener.stateUpdateHandler = nil
                    continuation.resume(returning: listener.port?.rawValue ?? 0)
                case .failed(let error):
                    listener.stateUpdateHandler = nil
                    continuation.resume(throwing: error)
                default: break
                }
            }
            listener.start(queue: .main)
        }
    }

    /// Reads one request; the one for the callback with a code or an error ends the wait.
    nonisolated private static func receive(_ connection: NWConnection, done: @escaping (Result<AuthRedirect, Error>) -> Void) {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 16384) { data, _, _, _ in
            let head = data.flatMap { String(data: $0, encoding: .utf8) }?.split(separator: "\r\n").first ?? ""
            let target = head.split(separator: " ").dropFirst().first.map(String.init) ?? ""
            let items = URLComponents(string: "http://localhost" + target)?.queryItems ?? []
            let value = { (name: String) in items.first { $0.name == name }?.value }
            let result: Result<AuthRedirect, Error>? = if let code = value("code"), let state = value("state") {
                .success(AuthRedirect(code: code, state: state, issuer: value("iss")))
            } else if let error = value("error") {
                .failure(Failed(errorDescription: value("error_description") ?? error))
            } else {
                nil
            }
            let body = result == nil ? "Waiting for sign-in." : "Signed in. You can go back to Beep."
            let reply = "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: \(body.utf8.count)\r\nConnection: close\r\n\r\n\(body)"
            connection.send(content: Data(reply.utf8), completion: .contentProcessed { _ in connection.cancel() })
            if let result { done(result) }
        }
    }
}
