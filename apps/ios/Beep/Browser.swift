import SwiftUI
import WebKit

/// The one in-app web view, shared by the browser screen and Beep's browser tool, as on Android.
/// It outlives the screen, so Beep can load pages before the screen shows and a page survives
/// closing and reopening it.
@MainActor
@Observable
final class PageBrowser: NSObject, WKNavigationDelegate {
    private(set) var url = ""
    private(set) var progress = 1.0
    private(set) var canGoBack = false
    private(set) var canGoForward = false

    @ObservationIgnored private var view: WKWebView?
    @ObservationIgnored private var watches: [NSKeyValueObservation] = []
    /// Counts navigations, so an action can tell whether it started a load.
    @ObservationIgnored private var loadsStarted = 0

    /// The web view for the screen to show, made on first use.
    var webView: WKWebView {
        if let view { return view }
        let config = WKWebViewConfiguration()
        config.websiteDataStore = .default()
        let made = WKWebView(frame: .zero, configuration: config)
        made.allowsBackForwardNavigationGestures = true
        made.navigationDelegate = self
        watches = [
            made.observe(\.url) { [weak self] web, _ in Task { @MainActor in self?.url = web.url?.absoluteString ?? "" } },
            made.observe(\.estimatedProgress) { [weak self] web, _ in Task { @MainActor in self?.progress = web.estimatedProgress } },
            made.observe(\.canGoBack) { [weak self] web, _ in Task { @MainActor in self?.canGoBack = web.canGoBack } },
            made.observe(\.canGoForward) { [weak self] web, _ in Task { @MainActor in self?.canGoForward = web.canGoForward } },
        ]
        view = made
        return made
    }

    /// Starts loading `target` without waiting, for links the user taps.
    func open(_ target: URL) {
        if webView.url != target { webView.load(URLRequest(url: target)) }
    }

    func goBack() { webView.goBack() }
    func goForward() { webView.goForward() }
    func reload() { webView.reload() }

    func webView(_ webView: WKWebView, didStartProvisionalNavigation navigation: WKNavigation!) {
        loadsStarted += 1
    }

    /// Only web pages load here; anything else (mail, phone, app links) goes to the system.
    func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction,
                 decisionHandler: @escaping @MainActor @Sendable (WKNavigationActionPolicy) -> Void) {
        guard let url = action.request.url, let scheme = url.scheme?.lowercased() else { return decisionHandler(.cancel) }
        if ["http", "https", "about", "blob", "data"].contains(scheme) { return decisionHandler(.allow) }
        decisionHandler(.cancel)
        UIApplication.shared.open(url)
    }

    // MARK: Beep's browser tool

    /// Runs one browser command and answers with JSON for the model, as Android does. Failures
    /// are answers too, with `error` and `message`, so the model can try something else.
    func run(_ command: BrowserCommand) async -> String {
        let out: [String: Any]
        do {
            out = try await perform(command)
        } catch {
            out = failure("browser_error", error.localizedDescription)
        }
        guard let data = try? JSONSerialization.data(withJSONObject: out),
              let json = String(data: data, encoding: .utf8) else {
            return #"{"error":"browser_error","message":"Could not encode the page."}"#
        }
        return json
    }

    private func perform(_ command: BrowserCommand) async throws -> [String: Any] {
        if case .navigate(let target) = command {
            guard let target = URL(string: target), target.scheme != nil else {
                return failure("bad_url", "Not a URL: \(target)")
            }
            webView.load(URLRequest(url: target))
            await awaitLoad()
            return pageInfo()
        }
        guard let web = view, web.url != nil else {
            return failure("no_browser_open", "No page is open. Use the navigate action with a URL first.")
        }
        switch command {
        case .navigate: return pageInfo()
        case .back:
            guard web.canGoBack else { return failure("no_history", "There is no earlier page to go back to.") }
            let before = loadsStarted
            web.goBack()
            await afterAction(before)
            return pageInfo()
        case .forward:
            guard web.canGoForward else { return failure("no_history", "There is no later page to go forward to.") }
            let before = loadsStarted
            web.goForward()
            await afterAction(before)
            return pageInfo()
        case .reload:
            web.reload()
            await awaitLoad()
            return pageInfo()
        case .getPageInfo:
            return pageInfo()
        case .readPage:
            guard let json = try await evaluate(Script.readPage) as? String,
                  let page = try JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any] else {
                return failure("read_failed", "Could not read the page.")
            }
            return page
        case .getText(let selector):
            guard let text = try await evaluate(Script.text, ["selector": selector]) as? String else {
                return notFound(selector: selector)
            }
            return ["text": text]
        case .click(let selector):
            return try await act(.selector(selector), verb: "click", done: ["clicked": true], Script.click)
        case .clickElement(let index):
            return try await act(.index(index), verb: "click", done: ["clicked": true], Script.click)
        case .type(let selector, let text):
            return try await act(.selector(selector), verb: "type", done: ["typed": true], Script.type,
                                 ["text": text, "submit": false])
        case .typeElement(let index, let text, let submit):
            return try await act(.index(index), verb: "type", done: ["typed": true], Script.type,
                                 ["text": text, "submit": submit])
        case .scroll(let direction):
            let up = direction == .up
            _ = try await evaluate("window.scrollBy(0, window.innerHeight * amount); return null;", ["amount": up ? -0.8 : 0.8])
            return ["scrolled": up ? "up" : "down"]
        case .waitFor(let selector):
            let deadline = Date.now.addingTimeInterval(10)
            while Date.now < deadline {
                if try await evaluate("return !!document.querySelector(selector);", ["selector": selector]) as? Bool == true {
                    return ["found": true]
                }
                try await Task.sleep(for: .milliseconds(250))
            }
            return notFound(selector: selector)
        }
    }

    private enum Target {
        case selector(String)
        case index(UInt32)
    }

    /// Runs `script` on the element `target` names, then waits for any page load it starts.
    private func act(_ target: Target, verb: String, done: [String: Any], _ script: String,
                     _ extra: [String: Any] = [:]) async throws -> [String: Any] {
        var args = extra
        let find: String
        switch target {
        case .selector(let selector):
            args["selector"] = selector
            find = "document.querySelector(selector)"
        case .index(let index):
            args["index"] = Int(index)
            find = "document.querySelector('[data-beep-idx=\"' + index + '\"]')"
        }
        let before = loadsStarted
        guard try await evaluate("const el = \(find);\n" + script, args) as? String == "ok" else {
            switch target {
            case .selector(let selector): return notFound(selector: selector)
            case .index(let index):
                return failure("element_not_found", "No element numbered \(index). Use read_page to get fresh numbers.")
            }
        }
        await afterAction(before)
        var out = done
        out["url"] = view?.url?.absoluteString ?? ""
        out["title"] = view?.title ?? ""
        out["note"] = "The page may have changed; read_page before the next \(verb)."
        return out
    }

    /// Waits for the load an action started, if it started one within a moment.
    private func afterAction(_ loadsBefore: Int) async {
        try? await Task.sleep(for: .milliseconds(400))
        if loadsStarted != loadsBefore { await awaitLoad() } else { await settle() }
    }

    /// Waits until the page has finished loading and stayed loaded for a moment, up to 15 seconds.
    private func awaitLoad() async {
        let deadline = Date.now.addingTimeInterval(15)
        while Date.now < deadline {
            if view?.isLoading == false {
                await settle()
                if view?.isLoading == false { return }
            } else {
                try? await Task.sleep(for: .milliseconds(100))
            }
        }
    }

    /// Gives scripts on the page a moment to render after a load or a click.
    private func settle() async {
        try? await Task.sleep(for: .milliseconds(500))
    }

    private func pageInfo() -> [String: Any] {
        ["url": view?.url?.absoluteString ?? "", "title": view?.title ?? "", "loading": view?.isLoading ?? false]
    }

    private func failure(_ code: String, _ message: String) -> [String: Any] {
        ["error": code, "message": message]
    }

    private func notFound(selector: String) -> [String: Any] {
        failure("element_not_found", "No element matches selector: \(selector)")
    }

    /// Runs `body` as a function in the page with `args` as its named arguments; nil for no value.
    private func evaluate(_ body: String, _ args: [String: Any] = [:]) async throws -> Any? {
        try await webView.callAsyncJavaScript(body, arguments: args, contentWorld: .page)
    }

    /// The page scripts, the same as Android's, written as function bodies.
    private enum Script {
        static let textLimit = 8000

        static let text = """
            const el = document.querySelector(selector);
            return el ? el.innerText.slice(0, \(textLimit)) : null;
            """

        static let click = """
            if (!el) return '';
            el.scrollIntoView({block: 'center'});
            if (el.focus) el.focus();
            el.click();
            return 'ok';
            """

        /// Sets a field's value the way frameworks like React notice, then submits if asked.
        static let type = """
            if (!el) return '';
            el.scrollIntoView({block: 'center'});
            el.focus();
            if (el.isContentEditable) {
              el.textContent = text;
            } else {
              const proto = el.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype
                : el.tagName === 'SELECT' ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
              Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, text);
            }
            el.dispatchEvent(new Event('input', {bubbles: true}));
            el.dispatchEvent(new Event('change', {bubbles: true}));
            if (submit) {
              const key = {key: 'Enter', code: 'Enter', keyCode: 13, which: 13, bubbles: true, cancelable: true};
              const go = el.dispatchEvent(new KeyboardEvent('keydown', key));
              el.dispatchEvent(new KeyboardEvent('keypress', key));
              el.dispatchEvent(new KeyboardEvent('keyup', key));
              if (go && el.form) { if (el.form.requestSubmit) el.form.requestSubmit(); else el.form.submit(); }
            }
            return 'ok';
            """

        /// Numbers the visible things Beep can act on and returns them with the page text.
        static let readPage = """
            const sel = 'a[href],button,input:not([type=hidden]),textarea,select,summary,[role=button],[role=link],' +
              '[role=tab],[role=menuitem],[role=checkbox],[role=option],[onclick],[contenteditable=true]';
            document.querySelectorAll('[data-beep-idx]').forEach(e => e.removeAttribute('data-beep-idx'));
            const lines = [];
            let n = 0;
            document.querySelectorAll(sel).forEach(el => {
              if (n >= 200) return;
              const r = el.getBoundingClientRect(), s = getComputedStyle(el);
              if (r.width < 1 || r.height < 1 || s.visibility === 'hidden' || s.display === 'none') return;
              el.setAttribute('data-beep-idx', n);
              const tag = el.tagName.toLowerCase();
              const label = (el.innerText || el.value || el.getAttribute('aria-label') || el.placeholder ||
                el.title || el.name || '').replace(/\\s+/g, ' ').trim().slice(0, 80);
              const extra = tag === 'a' ? el.getAttribute('href') : (tag === 'input' ? el.type : '');
              let line = '[' + n + '] ' + tag + (extra ? ' (' + String(extra).slice(0, 80) + ')' : '') + ' "' + label + '"';
              if (r.bottom < 0 || r.top > innerHeight) line += ' off screen';
              lines.push(line);
              n++;
            });
            const text = (document.body ? document.body.innerText : '').replace(/\\n{3,}/g, '\\n\\n');
            return JSON.stringify({
              url: location.href, title: document.title,
              text: text.slice(0, \(textLimit)), text_truncated: text.length > \(textLimit),
              elements: lines.join('\\n')
            });
            """
    }
}

/// The chat panel's height when the browser opens, and the least it drags down to.
private let shortPanel: CGFloat = 220

/// The shared web view full screen, with the chat in a panel below it that drags between short and tall.
struct BrowserView<Chat: View>: View {
    @Environment(AppModel.self) private var model
    let close: () -> Void
    @ViewBuilder let chat: () -> Chat
    @State private var panel = shortPanel
    @State private var dragStart: CGFloat?

    var body: some View {
        let browser = model.browser
        GeometryReader { geo in
            let tall = geo.size.height * 0.7
            VStack(spacing: 0) {
                bar(browser)
                ZStack(alignment: .bottomTrailing) {
                    WebViewHost(view: browser.webView)
                    if let url = URL(string: browser.url), url.scheme?.hasPrefix("http") == true {
                        Button { UIApplication.shared.open(url) } label: {
                            Label("Open in Safari", systemImage: "arrow.up.forward.square")
                                .font(.body(14, .semibold)).foregroundStyle(Mint.ink)
                                .padding(.horizontal, 14).padding(.vertical, 9)
                                .chunky(shape: Capsule())
                        }
                        .buttonStyle(.plain)
                        .padding(12)
                        .padding(.bottom, 14)
                    }
                }
                VStack(spacing: 0) {
                    Capsule().fill(Mint.soft).frame(width: 36, height: 4)
                        .frame(maxWidth: .infinity, minHeight: 24)
                        .contentShape(Rectangle())
                        .gesture(DragGesture(coordinateSpace: .global)
                            .onChanged { drag in
                                let start = dragStart ?? panel
                                dragStart = start
                                panel = min(max(start - drag.translation.height, shortPanel), tall)
                            }
                            .onEnded { _ in
                                dragStart = nil
                                withAnimation(.snappy) { panel = panel > (shortPanel + tall) / 2 ? tall : shortPanel }
                            })
                        .accessibilityLabel("Resize the chat")
                    chat()
                }
                .padding(.bottom, geo.safeAreaInsets.bottom)
                .frame(height: panel + geo.safeAreaInsets.bottom)
                .background(Mint.page)
                .clipShape(UnevenRoundedRectangle(topLeadingRadius: 18, topTrailingRadius: 18))
                .overlay(UnevenRoundedRectangle(topLeadingRadius: 18, topTrailingRadius: 18)
                    .stroke(Mint.ledge, lineWidth: 1.5))
                .padding(.top, -14)
            }
            // The panel runs down under the home indicator.
            .ignoresSafeArea(.container, edges: .bottom)
        }
        .background(Mint.page)
    }

    private func bar(_ browser: PageBrowser) -> some View {
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                icon("chevron.left", "Back", enabled: browser.canGoBack) { browser.goBack() }
                icon("chevron.right", "Forward", enabled: browser.canGoForward) { browser.goForward() }
                Text(browser.url.replacingOccurrences(of: "https://", with: ""))
                    .font(.body(12)).foregroundStyle(Mint.muted).lineLimit(1).truncationMode(.middle)
                    .padding(.horizontal, 10).padding(.vertical, 8)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(RoundedRectangle(cornerRadius: 8).fill(Mint.page))
                    .padding(.horizontal, 4)
                icon("arrow.clockwise", "Reload") { browser.reload() }
                icon("xmark", "Close", action: close)
            }
            .padding(.horizontal, 4).padding(.vertical, 4)
            if browser.progress < 1 {
                ProgressView(value: browser.progress).progressViewStyle(.linear).tint(Mint.brand).frame(height: 2)
            } else {
                Rectangle().fill(Mint.soft).frame(height: 1)
            }
        }
        .background(Mint.surface)
    }

    private func icon(_ name: String, _ label: String, enabled: Bool = true, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: name).font(.system(size: 17, weight: .medium)).foregroundStyle(Mint.ink)
                .frame(width: 44, height: 44).contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(!enabled)
        .opacity(enabled ? 1 : 0.35)
        .accessibilityLabel(label)
    }
}

/// Shows the shared web view; it is never torn down with the screen.
private struct WebViewHost: UIViewRepresentable {
    let view: WKWebView

    func makeUIView(context: Context) -> WKWebView { view }
    func updateUIView(_ view: WKWebView, context: Context) {}
}
