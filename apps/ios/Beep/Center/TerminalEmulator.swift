import SwiftTerm
import SwiftUI
import UIKit

/// Owns the emulator view, its key bar and the font size. The screen feeds it output and reads the
/// size it settles on; input and size changes go out through the closures.
@MainActor @Observable final class TerminalEmulatorModel: NSObject, TerminalViewDelegate {
    static let fontRange: ClosedRange<CGFloat> = 9...20
    static let background = UIColor(red: 0x0b / 255, green: 0x0c / 255, blue: 0x0e / 255, alpha: 1)
    static let foreground = UIColor(red: 0xd0 / 255, green: 0xd3 / 255, blue: 0xd8 / 255, alpha: 1)

    let view = SwiftTerm.TerminalView(frame: .zero)
    let keyBar = TerminalKeyBar()
    private(set) var cols = 0
    private(set) var rows = 0
    private var fontSize: CGFloat = 11
    private var pinchStart: CGFloat = 11
    private var readOnly = true

    /// Set while a stream is attached, cleared with `disconnect()`.
    private var onInput: (([UInt8]) -> Void)?
    private var onResize: ((Int, Int) -> Void)?
    private var sizeWaiter: (id: UUID, continuation: CheckedContinuation<(Int, Int)?, Never>)?

    override init() {
        super.init()
        view.terminalDelegate = self
        view.nativeBackgroundColor = Self.background
        view.nativeForegroundColor = Self.foreground
        view.font = UIFont.monospacedSystemFont(ofSize: fontSize, weight: .regular)
        keyBar.send = { [weak self] in self?.input($0) }
        view.addGestureRecognizer(UIPinchGestureRecognizer(target: self, action: #selector(pinch)))
    }

    /// The grid size once the view has been laid out, or nil if the caller is cancelled first.
    /// A newer call replaces an older waiter, which gets nil.
    func size() async -> (Int, Int)? {
        if cols > 0 { return (cols, rows) }
        let id = UUID()
        return await withTaskCancellationHandler {
            await withCheckedContinuation { continuation in
                if Task.isCancelled { continuation.resume(returning: nil); return }
                sizeWaiter?.continuation.resume(returning: nil)
                sizeWaiter = (id, continuation)
            }
        } onCancel: {
            Task { @MainActor in self.dropWaiter(id) }
        }
    }

    private func dropWaiter(_ id: UUID) {
        guard sizeWaiter?.id == id else { return }
        sizeWaiter?.continuation.resume(returning: nil)
        sizeWaiter = nil
    }

    /// Starts sending input and size changes. A resize that happened since `attachedAt` is sent now.
    func connect(attachedAt: (Int, Int), input: @escaping ([UInt8]) -> Void, resize: @escaping (Int, Int) -> Void) {
        onInput = input; onResize = resize
        if (cols, rows) != attachedAt, !readOnly { resize(cols, rows) }
    }

    func disconnect() { onInput = nil; onResize = nil }

    func feed(_ data: [UInt8]) { view.feed(byteArray: data[...]) }

    func reset() { view.getTerminal().resetToInitialState() }

    /// Hides the key bar and the keyboard, and sends nothing: no input and no resize. Output keeps
    /// coming. Leaving read-only sends the current grid once.
    func setReadOnly(_ value: Bool) {
        guard value != readOnly else { return }
        readOnly = value
        view.inputAccessoryView = value ? nil : keyBar
        view.reloadInputViews()
        if value { _ = view.resignFirstResponder() }
        else {
            _ = view.becomeFirstResponder()
            if cols > 0 { onResize?(cols, rows) }
        }
    }

    private func input(_ bytes: [UInt8]) { if !readOnly { onInput?(bytes) } }

    @objc private func pinch(_ gesture: UIPinchGestureRecognizer) {
        if gesture.state == .began { pinchStart = fontSize }
        let size = min(max((pinchStart * gesture.scale).rounded(), Self.fontRange.lowerBound), Self.fontRange.upperBound)
        guard size != fontSize else { return }
        fontSize = size
        view.font = UIFont.monospacedSystemFont(ofSize: size, weight: .regular)
    }

    nonisolated func sizeChanged(source: SwiftTerm.TerminalView, newCols: Int, newRows: Int) {
        MainActor.assumeIsolated {
            guard newCols > 0, newRows > 0, (newCols, newRows) != (cols, rows) else { return }
            cols = newCols; rows = newRows
            if let waiter = sizeWaiter { sizeWaiter = nil; waiter.continuation.resume(returning: (newCols, newRows)) }
            else if !readOnly { onResize?(newCols, newRows) }
        }
    }

    nonisolated func send(source: SwiftTerm.TerminalView, data: ArraySlice<UInt8>) {
        MainActor.assumeIsolated { input(keyBar.apply(Array(data))) }
    }

    nonisolated func setTerminalTitle(source: SwiftTerm.TerminalView, title: String) {}
    nonisolated func hostCurrentDirectoryUpdate(source: SwiftTerm.TerminalView, directory: String?) {}
    nonisolated func scrolled(source: SwiftTerm.TerminalView, position: Double) {}
    nonisolated func requestOpenLink(source: SwiftTerm.TerminalView, link: String, params: [String: String]) {
        guard let url = URL(string: link), url.scheme == "https" || url.scheme == "http" else { return }
        Task { @MainActor in UIApplication.shared.open(url) }
    }
    nonisolated func clipboardCopy(source: SwiftTerm.TerminalView, content: Data) {
        Task { @MainActor in UIPasteboard.general.string = String(data: content, encoding: .utf8) }
    }
    nonisolated func rangeChanged(source: SwiftTerm.TerminalView, startY: Int, endY: Int) {}
}

struct TerminalEmulator: UIViewRepresentable {
    let model: TerminalEmulatorModel

    func makeUIView(context: Context) -> SwiftTerm.TerminalView {
        model.view
    }

    func updateUIView(_ view: SwiftTerm.TerminalView, context: Context) {}
}

/// Sends typed bytes one after another, in order. A byte string that is not UTF-8 cannot travel as
/// text, so it is dropped rather than changed.
@MainActor final class TerminalWriter {
    private var continuation: AsyncStream<String>.Continuation?
    private var task: Task<Void, Never>?

    func start(send: @escaping (String) async throws -> Void, failed: @escaping (Error) -> Void) {
        stop()
        let (stream, continuation) = AsyncStream<String>.makeStream()
        self.continuation = continuation
        task = Task {
            for await text in stream {
                do { try await send(text) } catch { failed(error) }
            }
        }
    }

    func send(_ bytes: [UInt8]) {
        guard let text = String(bytes: bytes, encoding: .utf8) else { return }
        continuation?.yield(text)
    }

    func stop() {
        continuation?.finish(); continuation = nil
        task?.cancel(); task = nil
    }
}
