import Cocoa
import WebKit

let assets = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
let policy = CommandLine.arguments[2]
let bundle = FileHandle.standardInput.readDataToEndOfFile()
let html = """
<!doctype html><html><head><script src="boot.js"></script><script src="app.js" defer></script></head>
<body><pre><code class="language-mermaid">sequenceDiagram
 participant A as Desktop
 participant B as Phone
 A-&gt;&gt;B: Pair
</code></pre></body></html>
"""

final class Assets: NSObject, WKURLSchemeHandler {
    func webView(_ webView: WKWebView, start task: WKURLSchemeTask) {
        guard let url = task.request.url else { return }
        let path = url.path
        let data: Data
        let mime: String

        switch path {
        case "/index.html":
            data = Data(html.utf8)
            mime = "text/html"
        case "/boot.js":
            data = Data("window.results=[];addEventListener('message',e=>{if(e.data?.kind==='beep-diagram')results.push(e.data.action)});".utf8)
            mime = "text/javascript"
        case "/app.js":
            data = bundle
            mime = "text/javascript"
        case "/diagrams/renderer.js":
            guard let script = try? Data(contentsOf: assets.appendingPathComponent("renderer.js")) else {
                task.didFailWithError(NSError(domain: "assets", code: 404))
                return
            }
            let probe = """

            const forbidden = document.createElement('script');
            forbidden.src = 'tauri://localhost/unapproved.js';
            forbidden.onerror = () => parent.postMessage({kind:'beep-diagram',action:'probe-blocked'}, '*');
            forbidden.onload = () => parent.postMessage({kind:'beep-diagram',action:'probe-loaded'}, '*');
            document.head.append(forbidden);
            """
            data = script + Data(probe.utf8)
            mime = "text/javascript"
        case "/unapproved.js":
            data = Data("void 0".utf8)
            mime = "text/javascript"
        default:
            let file = assets.appendingPathComponent(String(path.dropFirst("/diagrams/".count)))
            guard path.hasPrefix("/diagrams/"), let read = try? Data(contentsOf: file) else {
                task.didFailWithError(NSError(domain: "assets", code: 404))
                return
            }
            data = read
            mime = file.pathExtension == "html" ? "text/html" : "text/javascript"
        }

        guard let response = HTTPURLResponse(url: url, statusCode: 200, httpVersion: nil, headerFields: [
            "Content-Type": mime, "Content-Security-Policy": policy, "Access-Control-Allow-Origin": "*",
        ]) else { return }
        task.didReceive(response)
        task.didReceive(data)
        task.didFinish()
    }

    func webView(_ webView: WKWebView, stop task: WKURLSchemeTask) {}
}

let app = NSApplication.shared
app.setActivationPolicy(.prohibited)
let config = WKWebViewConfiguration()
let handler = Assets()
config.setURLSchemeHandler(handler, forURLScheme: "tauri")
let web = WKWebView(frame: NSRect(x: 0, y: 0, width: 900, height: 700), configuration: config)
web.load(URLRequest(url: URL(string: "tauri://localhost/index.html")!))

var checks = 0
Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { timer in
    checks += 1
    web.evaluateJavaScript("results.includes('rendered') && results.includes('probe-blocked') && !results.includes('probe-loaded') && document.querySelector('iframe[title=\"Mermaid diagram\"]') && document.querySelector('pre').hidden") { value, _ in
        if value as? Bool == true {
            print("rendered")
            timer.invalidate()
            app.terminate(nil)
        } else if checks >= 40 {
            fputs("Diagram did not render or an unapproved script was allowed\n", stderr)
            exit(1)
        }
    }
}
app.run()
