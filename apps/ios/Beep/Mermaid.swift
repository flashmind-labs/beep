import SwiftUI
import WebKit

/// Mermaid source stays in the message. The local page renders and exports it offline.
struct MermaidDiagram: View {
    let source: String
    @State private var expanded = false
    @State private var height: CGFloat = 320
    @State private var share: SheetURL?
    @State private var fullShare: SheetURL?
    @State private var error = ""
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            page(expanded: false).frame(height: height)
            if !error.isEmpty { Text(error).font(.body(12)).foregroundStyle(Mint.muted) }
        }
        .fullScreenCover(isPresented: $expanded) {
            page(expanded: true).background(Mint.surface)
                .sheet(item: $fullShare) { item in ActivitySheet(items: [item.url]) }
        }
        .sheet(item: $share) { item in ActivitySheet(items: [item.url]) }
    }

    private func page(expanded full: Bool) -> some View {
        MermaidPage(source: source, expanded: full, dark: scheme == .dark) { message in
            switch message["action"] as? String {
            case "resize":
                if !full, let value = message["height"] as? Double, value.isFinite { height = min(700, max(120, value)) }
            case "expand": expanded = true
            case "close": expanded = false
            case "copy": UIPasteboard.general.string = source
            case "copy-image":
                guard message["mime"] as? String == "image/png", let encoded = message["data"] as? String,
                      encoded.count <= 24_000_000, let data = Data(base64Encoded: encoded),
                      let image = UIImage(data: data) else { error = "Could not copy image."; return }
                UIPasteboard.general.image = image
            case "save":
                guard let mime = message["mime"] as? String, ["image/png", "image/svg+xml"].contains(mime),
                      let encoded = message["data"] as? String, encoded.count <= 24_000_000,
                      let data = Data(base64Encoded: encoded) else { return }
                do {
                    let folder = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
                    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
                    let url = folder.appendingPathComponent(mime == "image/png" ? "diagram.png" : "diagram.svg")
                    try data.write(to: url)
                    if full { fullShare = SheetURL(url: url) }
                    else { share = SheetURL(url: url) }
                } catch { self.error = "Could not export diagram." }
            default: break
            }
        }
    }
}

private struct MermaidPage: UIViewRepresentable {
    let source: String
    let expanded: Bool
    let dark: Bool
    let receive: ([String: Any]) -> Void

    func makeCoordinator() -> Coordinator { Coordinator(self) }
    func makeUIView(context: Context) -> WKWebView {
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
        configuration.userContentController.add(context.coordinator, name: "diagram")
        let view = WKWebView(frame: .zero, configuration: configuration)
        view.navigationDelegate = context.coordinator
        view.isOpaque = false
        view.backgroundColor = .clear
        view.scrollView.isScrollEnabled = expanded
        view.scrollView.backgroundColor = .clear
        if let url = Bundle.main.url(forResource: "index", withExtension: "html", subdirectory: "assets") {
            view.loadFileURL(url, allowingReadAccessTo: url.deletingLastPathComponent())
        }
        return view
    }
    func updateUIView(_ view: WKWebView, context: Context) {
        context.coordinator.page = self
        if context.coordinator.loaded { context.coordinator.render(view) }
    }
    static func dismantleUIView(_ view: WKWebView, coordinator: Coordinator) {
        view.stopLoading()
        view.configuration.userContentController.removeScriptMessageHandler(forName: "diagram")
        view.navigationDelegate = nil
    }

    final class Coordinator: NSObject, WKNavigationDelegate, WKScriptMessageHandler {
        var page: MermaidPage
        var loaded = false
        private var rendered = ""
        init(_ page: MermaidPage) { self.page = page }
        func render(_ view: WKWebView) {
            let key = "\(page.expanded):\(page.dark):\(page.source)"
            guard rendered != key,
                  let data = try? JSONSerialization.data(withJSONObject: ["source": page.source, "expanded": page.expanded, "dark": page.dark]),
                  let json = String(data: data, encoding: .utf8) else { return }
            rendered = key
            view.evaluateJavaScript("window.renderDiagram(\(json))", completionHandler: nil)
        }
        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) { loaded = true; render(webView) }
        func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction,
                     decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
            // Only the initial bundled document may navigate; diagrams cannot open links.
            decisionHandler(!loaded && navigationAction.request.url?.isFileURL == true ? .allow : .cancel)
        }
        func userContentController(_ userContentController: WKUserContentController, didReceive message: WKScriptMessage) {
            guard message.frameInfo.isMainFrame, let body = message.body as? [String: Any], body["kind"] as? String == "beep-diagram" else { return }
            page.receive(body)
        }
    }
}
