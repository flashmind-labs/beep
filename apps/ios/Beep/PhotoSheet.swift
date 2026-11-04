import AVFoundation
import Photos
import PhotosUI
import SwiftUI

enum ChatPhotos {
    /// The most photos one message carries.
    static let limit = 8

    /// A photo as it is sent: at most 1024 px on its long side, as JPEG.
    static func jpeg(_ image: UIImage) -> Data? {
        let side = max(image.size.width, image.size.height)
        let scale = side > 1024 ? 1024 / side : 1
        let size = CGSize(width: (image.size.width * scale).rounded(), height: (image.size.height * scale).rounded())
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        let resized = UIGraphicsImageRenderer(size: size, format: format).image { _ in
            image.draw(in: CGRect(origin: .zero, size: size))
        }
        return resized.jpegData(compressionQuality: 0.8)
    }
}

/// A JPEG filling its frame.
struct PhotoThumb: View {
    let data: Data

    var body: some View {
        Mint.code.overlay {
            if let image = UIImage(data: data) {
                Image(uiImage: image).resizable().scaledToFill()
            }
        }
        .clipped()
    }
}

/// The image button's sheet: the camera first, then recent photos to pick, and the full gallery.
struct PhotoSheet: View {
    let room: Int
    let add: ([Data]) -> Void
    let camera: () -> Void
    let files: () -> Void
    @State private var recent: [PHAsset] = []
    @State private var picked: [PHAsset] = []
    @State private var gallery: [PhotosPickerItem] = []
    @State private var cameraAllowed = AVCaptureDevice.authorizationStatus(for: .video) == .authorized
    @State private var loading = false
    @Environment(\.openURL) private var openURL

    private let columns = Array(repeating: GridItem(.flexible(), spacing: 6), count: 3)

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            LazyVGrid(columns: columns, spacing: 6) {
                cameraTile
                ForEach(recent, id: \.localIdentifier) { asset in
                    recentTile(asset)
                }
            }
            if !cameraAllowed {
                Text("Beep only uses the camera while it's open here. Photos you take are sent with your message and not saved to the phone.")
                    .font(.body(12.5)).foregroundStyle(Mint.muted).lineSpacing(3)
            }
            HStack(spacing: 10) {
                PhotosPicker(selection: $gallery, maxSelectionCount: max(1, room), matching: .images) {
                    Label("All photos", systemImage: "photo").font(.body(14, .semibold))
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(PillButton())
                if !picked.isEmpty {
                    Button { addPicked() } label: {
                        Text(loading ? "Adding…" : "Add \(picked.count)").font(.body(14, .semibold)).frame(maxWidth: .infinity)
                    }
                    .buttonStyle(PillButton(fill: Mint.accent))
                    .disabled(loading)
                } else {
                    Button { files() } label: {
                        Label("Files", systemImage: "doc.text").font(.body(14, .semibold)).frame(maxWidth: .infinity)
                    }
                    .buttonStyle(PillButton())
                    .accessibilityLabel("Attach files")
                }
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, Mint.pageInset).padding(.top, 26)
        .background(Mint.page)
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
        .presentationCornerRadius(28)
        .task { await loadRecent() }
        .onChange(of: gallery) { Task { await addGallery() } }
    }

    private var cameraTile: some View {
        Button { openCamera() } label: {
            Color.clear.aspectRatio(1, contentMode: .fit)
                .overlay {
                    if cameraAllowed {
                        ZStack {
                            if let first = recent.first { AssetThumb(asset: first) }
                            Color.black.opacity(0.4)
                            Image(systemName: "camera").font(.system(size: 22, weight: .semibold)).foregroundStyle(.white)
                        }
                    } else {
                        VStack(spacing: 4) {
                            Image(systemName: "camera").font(.system(size: 20, weight: .semibold))
                            Text("Allow camera").font(.body(11))
                        }
                        .foregroundStyle(Mint.ink)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                        .background(Mint.tints[2])
                    }
                }
                .clipShape(RoundedRectangle(cornerRadius: Mint.smallRadius))
                .overlay(RoundedRectangle(cornerRadius: Mint.smallRadius).stroke(Mint.ledge, lineWidth: cameraAllowed ? 0 : 1.5))
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Take a photo")
    }

    private func recentTile(_ asset: PHAsset) -> some View {
        let number = picked.firstIndex(of: asset).map { $0 + 1 }
        return Button {
            if let index = picked.firstIndex(of: asset) {
                picked.remove(at: index)
            } else if picked.count < room {
                picked.append(asset)
            }
        } label: {
            Color.clear.aspectRatio(1, contentMode: .fit)
                .overlay { AssetThumb(asset: asset) }
                .clipShape(RoundedRectangle(cornerRadius: Mint.smallRadius))
                .overlay {
                    if number != nil {
                        RoundedRectangle(cornerRadius: Mint.smallRadius).strokeBorder(Mint.brand, lineWidth: 3)
                    }
                }
                .overlay(alignment: .topTrailing) {
                    if let number {
                        Text("\(number)").font(.body(11, .bold)).foregroundStyle(.white)
                            .frame(width: 20, height: 20).background(Circle().fill(Mint.brand)).padding(5)
                    }
                }
        }
        .buttonStyle(.plain)
        .accessibilityLabel(number.map { "Photo, picked \($0)" } ?? "Photo")
    }

    private func openCamera() {
        switch AVCaptureDevice.authorizationStatus(for: .video) {
        case .authorized: camera()
        case .notDetermined:
            Task {
                if await AVCaptureDevice.requestAccess(for: .video) {
                    cameraAllowed = true
                    camera()
                }
            }
        default:
            if let url = URL(string: UIApplication.openSettingsURLString) { openURL(url) }
        }
    }

    private func loadRecent() async {
        var status = PHPhotoLibrary.authorizationStatus(for: .readWrite)
        if status == .notDetermined, !UserDefaults.standard.bool(forKey: "noask") {
            status = await PHPhotoLibrary.requestAuthorization(for: .readWrite)
        }
        guard status == .authorized || status == .limited else { return }
        let options = PHFetchOptions()
        options.sortDescriptors = [NSSortDescriptor(key: "creationDate", ascending: false)]
        options.fetchLimit = 8
        let result = PHAsset.fetchAssets(with: .image, options: options)
        var assets: [PHAsset] = []
        result.enumerateObjects { asset, _, _ in assets.append(asset) }
        recent = assets
    }

    private func addPicked() {
        loading = true
        Task {
            var out: [Data] = []
            for asset in picked {
                if let image = await AssetThumb.image(asset, side: 1024), let jpeg = ChatPhotos.jpeg(image) { out.append(jpeg) }
            }
            loading = false
            add(out)
        }
    }

    private func addGallery() async {
        guard !gallery.isEmpty else { return }
        loading = true
        var out: [Data] = []
        for item in gallery.prefix(room) {
            if let data = try? await item.loadTransferable(type: Data.self), let image = UIImage(data: data),
                let jpeg = ChatPhotos.jpeg(image) {
                out.append(jpeg)
            }
        }
        loading = false
        add(out)
    }
}

/// A photo library thumbnail.
private struct AssetThumb: View {
    let asset: PHAsset
    @State private var image: UIImage?

    var body: some View {
        Mint.code.overlay {
            if let image { Image(uiImage: image).resizable().scaledToFill() }
        }
        .clipped()
        .task(id: asset.localIdentifier) { image = await Self.image(asset, side: 300) }
    }

    static func image(_ asset: PHAsset, side: CGFloat) async -> UIImage? {
        let options = PHImageRequestOptions()
        options.deliveryMode = .highQualityFormat
        options.isNetworkAccessAllowed = true
        return await withCheckedContinuation { done in
            PHImageManager.default().requestImage(
                for: asset, targetSize: CGSize(width: side, height: side), contentMode: .aspectFit, options: options
            ) { image, _ in done.resume(returning: image) }
        }
    }
}
