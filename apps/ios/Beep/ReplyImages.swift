import QuickLook
import SwiftUI
import UIKit

/// Files Beep sent, kept in Caches/sent/<id>/<name> so each downloads once. A sent image is
/// named `beep-file:<id>/<name>`, so the image views load it like one from the web.
enum SentFiles {
    private static let scheme = "beep-file:"

    static func imageURL(id: String, name: String) -> String {
        scheme + id + "/" + (name.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? "image")
    }

    static func isImage(_ url: String) -> Bool { url.hasPrefix(scheme) }

    /// The id and name an `imageURL` holds.
    static func parts(_ url: String) -> (id: String, name: String)? {
        guard isImage(url) else { return nil }
        let rest = url.dropFirst(scheme.count)
        let split = rest.split(separator: "/", maxSplits: 1).map(String.init)
        guard let id = split.first, !id.isEmpty else { return nil }
        return (id, split.count > 1 ? split[1].removingPercentEncoding ?? split[1] : "image")
    }

    static func local(id: String, name: String) -> URL {
        let caches = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
        let safe = name.replacingOccurrences(of: "/", with: "_")
        return caches.appendingPathComponent("sent/\(id.replacingOccurrences(of: "/", with: "_"))", isDirectory: true)
            .appendingPathComponent(safe.isEmpty ? "file" : safe)
    }
}

/// Reply images by url, decoded and kept in memory; a few large photos do not crowd out the rest.
@MainActor
final class ReplyImageCache {
    private let cache: NSCache<NSString, UIImage> = {
        let cache = NSCache<NSString, UIImage>()
        cache.totalCostLimit = 32 * 1024 * 1024
        return cache
    }()
    private nonisolated static let maxBytes = 10 * 1024 * 1024
    private nonisolated static let maxSide: CGFloat = 1600

    func cached(_ url: String) -> UIImage? { cache.object(forKey: url as NSString) }

    func load(_ url: String, model: AppModel) async -> UIImage? {
        if let image = cached(url) { return image }
        let data: Data?
        if url.hasPrefix("data:image/"), let comma = url.firstIndex(of: ",") {
            data = Data(base64Encoded: String(url[url.index(after: comma)...]), options: .ignoreUnknownCharacters)
        } else if url.hasPrefix("https://"), let link = URL(string: url) {
            data = await Self.download(link)
        } else if let file = SentFiles.parts(url) {
            data = (try? await model.sentFile(id: file.id, name: file.name)).flatMap { try? Data(contentsOf: $0) }
        } else {
            data = nil
        }
        guard let data, let image = await Self.decode(data) else { return nil }
        cache.setObject(image, forKey: url as NSString, cost: Int(image.size.width * image.size.height * image.scale * image.scale * 4))
        return image
    }

    private static func download(_ url: URL) async -> Data? {
        var request = URLRequest(url: url, timeoutInterval: 15)
        request.setValue("Beep for iOS", forHTTPHeaderField: "User-Agent")
        guard let (data, response) = try? await URLSession.shared.data(for: request),
              let http = response as? HTTPURLResponse, (200...299).contains(http.statusCode),
              data.count <= maxBytes else { return nil }
        return data
    }

    /// Scaled down so the longer side stays near `maxSide`.
    private static func decode(_ data: Data) async -> UIImage? {
        await Task.detached {
            guard let image = UIImage(data: data), image.size.width > 0, image.size.height > 0 else { return nil }
            let side = max(image.size.width, image.size.height)
            guard side > maxSide else { return image.preparingForDisplay() ?? image }
            let scale = maxSide / side
            return image.preparingThumbnail(of: CGSize(width: image.size.width * scale, height: image.size.height * scale)) ?? image
        }.value
    }
}

/// An image in a reply: what loads and what it says.
struct ReplyPicture: Hashable {
    var url: String
    var alt: String

    var web: Bool { url.hasPrefix("https://") || url.hasPrefix("http://") }
}

private enum Loaded: Equatable {
    case loading, failed, ok(UIImage)
}

/// An image Beep put in a reply. Tapping opens it full screen; one that fails to load becomes a link.
struct ReplyImage: View {
    @Environment(AppModel.self) private var model
    let picture: ReplyPicture
    @State private var state = Loaded.loading
    @State private var viewing = false

    var body: some View {
        SwiftUI.Group {
            switch state {
            case .ok(let image):
                Image(uiImage: image).resizable().scaledToFit()
                    .frame(maxWidth: .infinity, maxHeight: 360, alignment: .leading)
                    .clipShape(RoundedRectangle(cornerRadius: 12))
                    .onTapGesture { viewing = true }
                    .accessibilityLabel(picture.alt.isEmpty ? "Image" : picture.alt)
                    .accessibilityAddTraits(.isButton)
                    .fullScreenCover(isPresented: $viewing) { ImageViewer(images: [Shown(picture: picture, image: image)], start: 0) }
            case .loading:
                RoundedRectangle(cornerRadius: 12).fill(Mint.code).frame(maxWidth: .infinity, minHeight: 140)
            case .failed:
                ImageLink(picture: picture)
            }
        }
        .task(id: picture.url) {
            if let image = model.replyImages.cached(picture.url) { state = .ok(image); return }
            state = await model.replyImages.load(picture.url, model: model).map(Loaded.ok) ?? .failed
        }
    }
}

/// Images on consecutive lines as one row that scrolls sideways, each as tall as the others.
/// One that fails drops out; if all fail the row becomes their links.
struct ReplyGallery: View {
    @Environment(AppModel.self) private var model
    let pictures: [ReplyPicture]
    @State private var states: [String: Loaded] = [:]
    @State private var viewing: Int?
    private let height: CGFloat = 200

    var body: some View {
        if !pictures.isEmpty, pictures.allSatisfy({ states[$0.url] == .failed }) {
            VStack(alignment: .leading, spacing: 2) { ForEach(pictures, id: \.self) { ImageLink(picture: $0) } }
        } else {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(alignment: .top, spacing: 8) {
                    ForEach(pictures, id: \.self) { picture in tile(picture) }
                }
            }
            .task(id: pictures) {
                await withTaskGroup(of: Void.self) { group in
                    for picture in pictures {
                        group.addTask { @MainActor in
                            let image = await model.replyImages.load(picture.url, model: model)
                            states[picture.url] = image.map(Loaded.ok) ?? .failed
                        }
                    }
                }
            }
            .fullScreenCover(isPresented: Binding(get: { viewing != nil }, set: { if !$0 { viewing = nil } })) {
                ImageViewer(images: shown, start: viewing ?? 0)
            }
        }
    }

    private var shown: [Shown] {
        pictures.compactMap { picture in
            if case .ok(let image) = states[picture.url] { return Shown(picture: picture, image: image) }
            return nil
        }
    }

    @ViewBuilder private func tile(_ picture: ReplyPicture) -> some View {
        switch states[picture.url] ?? .loading {
        case .failed:
            EmptyView()
        case .loading:
            RoundedRectangle(cornerRadius: 12).fill(Mint.code).frame(width: height * 1.3, height: height)
        case .ok(let image):
            let ratio = min(max(image.size.width / image.size.height, 0.5), 1.8)
            VStack(alignment: .leading, spacing: 4) {
                Image(uiImage: image).resizable().scaledToFill()
                    .frame(width: height * ratio, height: height)
                    .clipShape(RoundedRectangle(cornerRadius: 12))
                    .contentShape(Rectangle())
                    .onTapGesture { viewing = shown.firstIndex { $0.picture == picture } }
                if !picture.alt.isEmpty {
                    Text(picture.alt).font(.body(12)).foregroundStyle(Mint.muted).lineLimit(2)
                }
            }
            .frame(width: height * ratio)
        }
    }
}

/// What is left of an image that did not load: its description, a link when it came from the web.
private struct ImageLink: View {
    @Environment(\.openURL) private var openURL
    let picture: ReplyPicture

    var body: some View {
        let label = picture.alt.isEmpty ? (picture.web ? picture.url : "Image") : picture.alt
        if picture.web, let url = URL(string: picture.url) {
            Button { openURL(url) } label: { Text(label).underline() }
                .buttonStyle(.plain).font(.body(16)).foregroundStyle(Mint.brand)
        } else {
            Text(label).font(.body(16)).foregroundStyle(Mint.brand)
        }
    }
}

struct Shown: Equatable {
    var picture: ReplyPicture
    var image: UIImage
}

/// Full screen images, swiped through in order, with Open for one from the web and Share for one Beep sent.
struct ImageViewer: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @Environment(\.openURL) private var openURL
    let images: [Shown]
    let start: Int
    @State private var page = 0
    @State private var file: URL?

    var body: some View {
        let current = images.indices.contains(page) ? images[page].picture : nil
        ZStack(alignment: .top) {
            Color.black.ignoresSafeArea()
            TabView(selection: $page) {
                ForEach(images.indices, id: \.self) { index in
                    Image(uiImage: images[index].image).resizable().scaledToFit()
                        .accessibilityLabel(images[index].picture.alt.isEmpty ? "Image" : images[index].picture.alt)
                        .tag(index)
                }
            }
            .tabViewStyle(.page(indexDisplayMode: .never))
            .ignoresSafeArea()
            HStack(spacing: 12) {
                Button { dismiss() } label: {
                    Image(systemName: "xmark").font(.system(size: 18, weight: .semibold)).frame(width: 44, height: 44)
                }
                .accessibilityLabel("Close")
                if images.count > 1 { Text("\(page + 1) of \(images.count)").font(.body(14)) }
                Spacer()
                if let current, current.web, let url = URL(string: current.url) {
                    Button("Open") { openURL(url) }.font(.body(15, .semibold))
                }
                if let file { ShareLink(item: file) { Text("Share").font(.body(15, .semibold)) } }
            }
            .foregroundStyle(.white)
            .padding(.horizontal, 8)
            if let current, !current.alt.isEmpty {
                VStack {
                    Spacer()
                    Text(current.alt).font(.body(14)).foregroundStyle(.white)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal, 16).padding(.vertical, 12)
                        .background(Color.black.opacity(0.5))
                }
            }
        }
        .onAppear { page = start }
        .task(id: page) {
            file = nil
            guard let current, let sent = SentFiles.parts(current.url) else { return }
            file = try? await model.sentFile(id: sent.id, name: sent.name)
        }
    }
}

/// A file a send_file call sent.
struct SentFile: Equatable {
    var id: String
    var name: String
    var mime: String
    var size: Int64
}

/// A file Beep sent. Tapping downloads it once and opens it; the share button hands it to another app.
struct FileCard: View {
    @Environment(AppModel.self) private var model
    let file: SentFile
    @State private var loading = false
    @State private var gone = false
    @State private var error = ""
    @State private var preview: URL?
    @State private var sharing: URL?

    var body: some View {
        HStack(spacing: 12) {
            Button { withFile { preview = $0 } } label: {
                HStack(spacing: 12) {
                    Image(systemName: "doc.text").font(.system(size: 18)).foregroundStyle(Mint.ink)
                        .frame(width: 40, height: 40)
                        .background(RoundedRectangle(cornerRadius: 10).fill(Mint.tints[0]))
                    VStack(alignment: .leading, spacing: 2) {
                        Text(file.name).font(.body(14, .semibold)).foregroundStyle(Mint.ink).lineLimit(1)
                        Text(detail).font(.body(12)).foregroundStyle(error.isEmpty ? Mint.muted : Mint.error).lineLimit(1)
                    }
                    Spacer(minLength: 0)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .disabled(gone)
            if loading {
                ProgressView().frame(width: 44, height: 44)
            } else if !gone {
                Button { withFile { sharing = $0 } } label: {
                    Image(systemName: "square.and.arrow.up").font(.system(size: 16)).foregroundStyle(Mint.ink)
                        .frame(width: 44, height: 44)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Share")
            }
        }
        .padding(.leading, 12).padding(.trailing, 4).padding(.vertical, 8)
        .background(RoundedRectangle(cornerRadius: 12).fill(Mint.surface))
        .overlay(RoundedRectangle(cornerRadius: 12).stroke(Mint.soft, lineWidth: 1))
        .opacity(gone ? 0.6 : 1)
        .quickLookPreview($preview)
        .sheet(item: Binding(get: { sharing.map(SheetURL.init) }, set: { sharing = $0?.url })) { item in
            ActivitySheet(items: [item.url]).presentationDetents([.medium, .large])
        }
    }

    private var detail: String {
        if gone { return "No longer available" }
        if loading { return "Downloading..." }
        if !error.isEmpty { return error }
        let kind = file.mime == "application/pdf" ? "PDF" : String((file.name as NSString).pathExtension.uppercased().prefix(5))
        let size = file.size > 0 ? ByteCountFormatter.string(fromByteCount: file.size, countStyle: .file) : ""
        return [kind, size].filter { !$0.isEmpty }.joined(separator: ", ")
    }

    private func withFile(_ then: @escaping (URL) -> Void) {
        guard !loading, !gone else { return }
        loading = true
        Task {
            do {
                then(try await model.sentFile(id: file.id, name: file.name))
                error = ""
            } catch BeepError.NotFound {
                gone = true
            } catch {
                self.error = describe(error)
            }
            loading = false
        }
    }
}

struct SheetURL: Identifiable {
    let url: URL
    var id: URL { url }
}

extension View {
    /// The page in Safari inside the app while `url` is set.
    func safariSheet(_ url: Binding<URL?>) -> some View {
        sheet(item: Binding(get: { url.wrappedValue.map(SheetURL.init) }, set: { url.wrappedValue = $0?.url })) {
            SafariView(url: $0.url).ignoresSafeArea()
        }
    }
}

/// The system share sheet.
struct ActivitySheet: UIViewControllerRepresentable {
    let items: [Any]

    func makeUIViewController(context: Context) -> UIActivityViewController {
        UIActivityViewController(activityItems: items, applicationActivities: nil)
    }

    func updateUIViewController(_ controller: UIActivityViewController, context: Context) {}
}
