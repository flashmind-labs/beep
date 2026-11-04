import SwiftUI
import WebKit

/// The one in-app web view for links the user opens, as on Android. It outlives the screen, so a
/// page survives closing and reopening it.
@MainActor
@Observable
final class PageBrowser: NSObject, WKNavigationDelegate {
    private(set) var url = ""
    private(set) var progress = 1.0
    private(set) var canGoBack = false
    private(set) var canGoForward = false

    @ObservationIgnored private var view: WKWebView?
    @ObservationIgnored private var watches: [NSKeyValueObservation] = []

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

    /// Only web pages load here; anything else (mail, phone, app links) goes to the system.
    func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction,
                 decisionHandler: @escaping @MainActor @Sendable (WKNavigationActionPolicy) -> Void) {
        guard let url = action.request.url, let scheme = url.scheme?.lowercased() else { return decisionHandler(.cancel) }
        if ["http", "https", "about", "blob", "data"].contains(scheme) { return decisionHandler(.allow) }
        decisionHandler(.cancel)
        UIApplication.shared.open(url)
    }
}

/// The shared web view full screen, under a bar with back, forward, reload and close.
struct BrowserView: View {
    @Environment(AppModel.self) private var model
    let close: () -> Void

    var body: some View {
        let browser = model.browser
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
                }
            }
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
                    .background(RoundedRectangle(cornerRadius: Mint.smallRadius).fill(Mint.page))
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
