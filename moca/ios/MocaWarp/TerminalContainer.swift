import SwiftTerm
import SwiftUI
import UIKit

/// Envuelve el `TerminalView` de SwiftTerm y lo conecta con la sesión remota.
struct TerminalContainer: UIViewRepresentable {
    @ObservedObject var session: RemoteSession

    func makeCoordinator() -> Coordinator {
        Coordinator(session: session)
    }

    func makeUIView(context: Context) -> TerminalView {
        let view = TerminalView(frame: .zero)
        view.terminalDelegate = context.coordinator
        view.backgroundColor = .black
        view.nativeBackgroundColor = .black
        view.nativeForegroundColor = UIColor(white: 0.92, alpha: 1)
        view.font = UIFont.monospacedSystemFont(ofSize: 15, weight: .regular)
        context.coordinator.terminal = view

        session.onOutput = { [weak view] bytes in
            view?.feed(byteArray: bytes[...])
        }
        // Conecta después del primer layout: para entonces `sizeChanged` ya informó el tamaño
        // real y la URL de /create lo lleva.
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
            session.connect()
            _ = view.becomeFirstResponder()
        }
        return view
    }

    func updateUIView(_ uiView: TerminalView, context: Context) {}

    final class Coordinator: NSObject, TerminalViewDelegate {
        let session: RemoteSession
        weak var terminal: TerminalView?

        init(session: RemoteSession) {
            self.session = session
        }

        func send(source: TerminalView, data: ArraySlice<UInt8>) {
            Task { @MainActor in session.send(data) }
        }

        func sizeChanged(source: TerminalView, newCols: Int, newRows: Int) {
            Task { @MainActor in session.resize(cols: newCols, rows: newRows) }
        }

        func setTerminalTitle(source: TerminalView, title: String) {}

        func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {}

        func scrolled(source: TerminalView, position: Double) {}

        func requestOpenLink(source: TerminalView, link: String, params: [String: String]) {
            if let url = URL(string: link) {
                UIApplication.shared.open(url)
            }
        }

        func bell(source: TerminalView) {}

        func clipboardCopy(source: TerminalView, content: Data) {
            if let text = String(data: content, encoding: .utf8) {
                UIPasteboard.general.string = text
            }
        }

        func iTermContent(source: TerminalView, content: ArraySlice<UInt8>) {}

        func rangeChanged(source: TerminalView, startY: Int, endY: Int) {}
    }
}
