import SwiftUI
import UIKit

/// A picture the user sent: a data URI sent from this phone, or a file kept on the server.
struct Picture: Identifiable, Equatable, Hashable {
    var fileId: String?
    var dataUri: String?
    var width: UInt32 = 0
    var height: UInt32 = 0
    /// The kept file's type, so a rerun can send it again as it was.
    var mime = "image/jpeg"

    var id: String { fileId ?? dataUri.map { String($0.hashValue) } ?? "" }

    init(_ ref: ImageRef) {
        fileId = ref.id
        width = ref.width
        height = ref.height
        mime = ref.mime
    }

    init(dataUri: String) {
        self.dataUri = dataUri
    }
}

/// Pictures by the message they came with, from get_session_images or get_group_images.
func picturesByMessage(_ sent: [SentImages]) -> [Int64: [Picture]] {
    var out: [Int64: [Picture]] = [:]
    for entry in sent { out[entry.message, default: []] += entry.images.map(Picture.init) }
    return out
}

/// Server pictures cached under Caches/pictures/<id>, fetched once when the file is missing.
@MainActor
final class PictureCache {
    private var loading: [String: Task<URL?, Never>] = [:]

    private static var folder: URL {
        let caches = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
        return caches.appendingPathComponent("pictures", isDirectory: true)
    }

    /// The local file for a server picture, or nil when it could not be fetched.
    func file(_ id: String, client: BeepClient?) async -> URL? {
        let name = id.replacingOccurrences(of: "/", with: "_")
        let url = Self.folder.appendingPathComponent(name)
        if FileManager.default.fileExists(atPath: url.path) { return url }
        if let running = loading[id] { return await running.value }
        guard let client else { return nil }
        let task = Task<URL?, Never> {
            try? FileManager.default.createDirectory(at: Self.folder, withIntermediateDirectories: true)
            // The bridge writes to a temporary name and renames it, so a failure leaves nothing at `url`.
            do {
                _ = try await client.saveFile(id: id, path: url.path)
                return url
            } catch {
                return nil
            }
        }
        loading[id] = task
        let result = await task.value
        loading[id] = nil
        return result
    }
}

/// Decodes the image in a `data:image/...;base64,` URI.
private func decodeDataUri(_ uri: String) -> UIImage? {
    guard let comma = uri.firstIndex(of: ","),
        let data = Data(base64Encoded: String(uri[uri.index(after: comma)...]), options: .ignoreUnknownCharacters)
    else { return nil }
    return UIImage(data: data)
}

/// One picture in a bubble: a plain tile while it loads, the picture, or a dashed
/// tile saying it is gone when the server no longer has it.
struct PictureTile: View {
    @Environment(AppModel.self) private var model
    let picture: Picture
    var height: CGFloat = 130

    private enum Status: Equatable {
        case loading, failed
        case loaded(UIImage)
    }

    @State private var status = Status.loading

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 14)
        ZStack {
            switch status {
            case .loading:
                shape.fill(Mint.code)
            case .failed:
                Text("Picture no longer available")
                    .font(.body(11)).foregroundStyle(Mint.muted)
                    .multilineTextAlignment(.center)
                    .padding(6)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .overlay(shape.strokeBorder(Mint.muted.opacity(0.6), style: StrokeStyle(lineWidth: 1, dash: [4, 3])))
            case .loaded(let image):
                Color.clear.overlay(Image(uiImage: image).resizable().scaledToFill())
            }
        }
        .frame(maxWidth: .infinity)
        .frame(height: height)
        .clipShape(shape)
        .accessibilityLabel(status == .failed ? "Picture no longer available" : "Picture")
        .task(id: picture) { await load() }
    }

    private func load() async {
        if let uri = picture.dataUri {
            status = decodeDataUri(uri).map(Status.loaded) ?? .failed
            return
        }
        guard let id = picture.fileId, let url = await model.pictureFile(id),
            let image = await Task.detached(operation: { UIImage(contentsOfFile: url.path) }).value
        else {
            status = .failed
            return
        }
        status = .loaded(image)
    }
}

/// The pictures at the top of a sent bubble: one wide, or two to a row as in the mock.
struct PictureGrid: View {
    let pictures: [Picture]

    var body: some View {
        if pictures.count == 1, let only = pictures.first {
            PictureTile(picture: only, height: singleHeight(only))
        } else {
            LazyVGrid(columns: [GridItem(.flexible(), spacing: 4), GridItem(.flexible(), spacing: 4)], spacing: 4) {
                ForEach(pictures) { PictureTile(picture: $0) }
            }
        }
    }

    /// Keeps the picture's shape, within limits, across the bubble's width.
    private func singleHeight(_ picture: Picture) -> CGFloat {
        guard picture.width > 0, picture.height > 0 else { return 160 }
        return min(280, max(120, 212 * CGFloat(picture.height) / CGFloat(picture.width)))
    }
}
