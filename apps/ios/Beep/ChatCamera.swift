import AVFoundation
import SwiftUI

/// The camera inside the chat. Each shot adds to the count, and Done puts them all in the composer.
/// Photos are not saved to the phone.
struct ChatCamera: View {
    let room: Int
    let done: ([Data]) -> Void
    @State private var camera = CameraSession()
    @State private var shots: [Data] = []
    @State private var reviewing = false

    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()
            if reviewing, let last = shots.last {
                review(last)
            } else {
                viewfinder
            }
        }
        .edgeSwipeBack { done(shots) }
        .preferredColorScheme(.dark)
        .statusBarHidden()
        .task { camera.start() }
        .onDisappear { camera.stop() }
    }

    private var viewfinder: some View {
        VStack(spacing: 0) {
            ZStack {
                if camera.available {
                    CameraPreview(session: camera.session).ignoresSafeArea(edges: .top)
                } else {
                    Text("No camera on this device").font(.body(14)).foregroundStyle(.white.opacity(0.7))
                }
                LinearGradient(stops: [.init(color: .black.opacity(0.5), location: 0), .init(color: .clear, location: 0.18),
                                       .init(color: .clear, location: 0.7), .init(color: .black.opacity(0.6), location: 1)],
                               startPoint: .top, endPoint: .bottom)
                    .ignoresSafeArea(edges: .top)
                    .allowsHitTesting(false)
                VStack {
                    HStack {
                        iconButton("xmark", "Close camera") { done(shots) }
                        Spacer()
                        iconButton(camera.flash ? "bolt.fill" : "bolt.slash", camera.flash ? "Flash on" : "Flash off") {
                            camera.flash.toggle()
                        }
                    }
                    .padding(.horizontal, 8)
                    Spacer()
                    if camera.available { zoomRow.padding(.bottom, 12) }
                }
            }
            HStack {
                ZStack(alignment: .bottomTrailing) {
                    if let last = shots.last {
                        PhotoThumb(data: last).frame(width: 48, height: 48)
                            .clipShape(RoundedRectangle(cornerRadius: 12))
                            .overlay(RoundedRectangle(cornerRadius: 12).stroke(.white, lineWidth: 2))
                        Text("\(shots.count)").font(.body(11, .bold)).foregroundStyle(Mint.ink)
                            .frame(minWidth: 18, minHeight: 18).background(Capsule().fill(Mint.accent)).padding(2)
                    }
                }
                .frame(width: 48, height: 48)
                .onTapGesture { if !shots.isEmpty { reviewing = true } }
                .accessibilityLabel("\(shots.count) photos taken")
                Spacer()
                Button {
                    camera.capture { data in
                        guard let data else { return }
                        shots.append(data)
                        reviewing = true
                    }
                } label: {
                    Circle().fill(.white).frame(width: 58, height: 58)
                        .padding(5)
                        .overlay(Circle().stroke(.white, lineWidth: 4))
                }
                .buttonStyle(.plain)
                .disabled(!camera.available || shots.count >= room)
                .accessibilityLabel("Take photo")
                Spacer()
                Button { camera.flip() } label: {
                    Image(systemName: "arrow.triangle.2.circlepath").font(.system(size: 20, weight: .semibold))
                        .foregroundStyle(.white).frame(width: 48, height: 48)
                        .background(Circle().fill(.white.opacity(0.15)))
                }
                .buttonStyle(.plain)
                .disabled(!camera.available)
                .accessibilityLabel("Switch camera")
            }
            .padding(.horizontal, 28).padding(.top, 18).padding(.bottom, 26)
            .overlay(alignment: .topTrailing) {
                if !shots.isEmpty {
                    Button("Done") { done(shots) }
                        .font(.body(14, .semibold)).foregroundStyle(.white)
                        .padding(.horizontal, 14).frame(minHeight: 36)
                        .background(Capsule().fill(.black.opacity(0.5)))
                        .padding(.trailing, 16).offset(y: -48)
                }
            }
        }
    }

    private var zoomRow: some View {
        HStack(spacing: 16) {
            ForEach(camera.zooms, id: \.self) { zoom in
                Button { camera.zoom = zoom } label: {
                    Text(zoom == 0.5 ? "0.5×" : "\(Int(zoom))×").font(.body(13, .semibold))
                        .foregroundStyle(camera.zoom == zoom ? Mint.accent : .white.opacity(0.6))
                        .frame(minWidth: 36, minHeight: 32)
                }
                .buttonStyle(.plain)
            }
        }
    }

    private func review(_ shot: Data) -> some View {
        VStack(spacing: 0) {
            HStack {
                iconButton("arrow.left", "Back to the camera") { reviewing = false }
                Text("Photo \(shots.count)").font(.body(14)).foregroundStyle(.white.opacity(0.8))
                Spacer()
            }
            .padding(.leading, 6)
            if let image = UIImage(data: shot) {
                Image(uiImage: image).resizable().scaledToFit().frame(maxWidth: .infinity, maxHeight: .infinity)
                    .padding(.bottom, 8)
            }
            HStack(spacing: 10) {
                Button {
                    shots.removeLast()
                    reviewing = false
                } label: { Text("Retake").frame(maxWidth: .infinity) }
                    .buttonStyle(PillButton(fill: .clear))
                    .foregroundStyle(.white)
                Button { done(shots) } label: {
                    Text(shots.count == 1 ? "Use photo" : "Use \(shots.count) photos").font(.body(14, .semibold))
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(PillButton(fill: Mint.accent))
            }
            .padding(.horizontal, 16).padding(.top, 8).padding(.bottom, 22)
        }
    }

    private func iconButton(_ icon: String, _ label: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: icon).font(.system(size: 18, weight: .semibold))
                .foregroundStyle(.white).frame(width: 44, height: 44).contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(label)
    }
}

/// The capture session behind the camera screen.
@MainActor
@Observable
final class CameraSession: NSObject, AVCapturePhotoCaptureDelegate {
    @ObservationIgnored let session = AVCaptureSession()
    @ObservationIgnored private let output = AVCapturePhotoOutput()
    @ObservationIgnored private var input: AVCaptureDeviceInput?
    @ObservationIgnored private var taken: ((Data?) -> Void)?
    private(set) var available = false
    private(set) var zooms: [CGFloat] = [1]
    var flash = false
    var zoom: CGFloat = 1 {
        didSet { applyZoom() }
    }

    func start() {
        guard input == nil else { return }
        session.beginConfiguration()
        session.sessionPreset = .photo
        if session.canAddOutput(output) { session.addOutput(output) }
        session.commitConfiguration()
        use(.back)
        let session = session
        Task.detached { session.startRunning() }
    }

    func stop() {
        let session = session
        Task.detached { session.stopRunning() }
    }

    func flip() {
        use(input?.device.position == .back ? .front : .back)
    }

    /// The back camera that covers the most lenses, so 0.5x works where the phone has an ultra wide.
    private func use(_ position: AVCaptureDevice.Position) {
        let types: [AVCaptureDevice.DeviceType] = position == .back
            ? [.builtInTripleCamera, .builtInDualWideCamera, .builtInWideAngleCamera] : [.builtInWideAngleCamera]
        let found = AVCaptureDevice.DiscoverySession(deviceTypes: types, mediaType: .video, position: position).devices
        guard let device = types.lazy.compactMap({ type in found.first { $0.deviceType == type } }).first,
            let next = try? AVCaptureDeviceInput(device: device) else { return }
        session.beginConfiguration()
        if let input { session.removeInput(input) }
        if session.canAddInput(next) {
            session.addInput(next)
            input = next
        }
        session.commitConfiguration()
        available = input != nil
        zooms = wide > 1 ? [0.5, 1, 2] : [1, 2]
        zoom = 1
    }

    /// The device zoom factor that shows as 1x: past the ultra wide on a multi-lens camera.
    private var wide: CGFloat {
        guard let device = input?.device, device.isVirtualDevice,
            let first = device.virtualDeviceSwitchOverVideoZoomFactors.first else { return 1 }
        return CGFloat(truncating: first)
    }

    private func applyZoom() {
        guard let device = input?.device, (try? device.lockForConfiguration()) != nil else { return }
        let factor = wide > 1 ? zoom * wide : zoom
        device.videoZoomFactor = min(max(factor, device.minAvailableVideoZoomFactor), device.maxAvailableVideoZoomFactor)
        device.unlockForConfiguration()
    }

    func capture(_ done: @escaping (Data?) -> Void) {
        guard available else { return done(nil) }
        let settings = AVCapturePhotoSettings()
        if output.supportedFlashModes.contains(.on) { settings.flashMode = flash ? .on : .off }
        taken = done
        output.capturePhoto(with: settings, delegate: self)
    }

    nonisolated func photoOutput(_ output: AVCapturePhotoOutput, didFinishProcessingPhoto photo: AVCapturePhoto,
                                 error: Error?) {
        let data = photo.fileDataRepresentation()
        Task { @MainActor in
            let jpeg = data.flatMap(UIImage.init(data:)).flatMap(ChatPhotos.jpeg)
            self.taken?(jpeg)
            self.taken = nil
        }
    }
}

private struct CameraPreview: UIViewRepresentable {
    let session: AVCaptureSession

    func makeUIView(context: Context) -> PreviewView {
        let view = PreviewView()
        view.preview.session = session
        view.preview.videoGravity = .resizeAspectFill
        return view
    }

    func updateUIView(_ view: PreviewView, context: Context) {}

    final class PreviewView: UIView {
        override class var layerClass: AnyClass { AVCaptureVideoPreviewLayer.self }
        var preview: AVCaptureVideoPreviewLayer {
            guard let layer = layer as? AVCaptureVideoPreviewLayer else { return AVCaptureVideoPreviewLayer() }
            return layer
        }
    }
}
