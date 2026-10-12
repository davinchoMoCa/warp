import Foundation

/// Dirección del proxy y token. Salen de `Config.generated.plist`, que arma `build.sh` a partir de
/// `~/.config/moca/remote-tty.env` de la Mac: nunca van en el repo (es público).
struct RemoteConfig {
    var url: URL?
    var token: String

    static func load() -> RemoteConfig {
        guard let path = Bundle.main.path(forResource: "Config.generated", ofType: "plist"),
              let dict = NSDictionary(contentsOfFile: path) as? [String: String]
        else {
            return RemoteConfig(url: nil, token: "")
        }
        return RemoteConfig(url: dict["url"].flatMap(URL.init(string:)), token: dict["token"] ?? "")
    }
}

/// Conexión websocket con moca-pty-proxy: bytes de la terminal en binario, redimensionado en JSON.
@MainActor
final class RemoteSession: ObservableObject {
    enum State: Equatable {
        case idle
        case connecting
        case connected
        case disconnected(String)

        var message: String {
            switch self {
            case .idle, .connecting: return "Conectando con la XPS…"
            case .connected: return ""
            case .disconnected(let reason): return reason
            }
        }

        var canRetry: Bool {
            if case .disconnected = self { return true }
            return false
        }
    }

    @Published private(set) var state: State = .idle

    /// Bytes que llegan del shell remoto; la vista de terminal los dibuja.
    var onOutput: (([UInt8]) -> Void)?

    private let config: RemoteConfig
    private var task: URLSessionWebSocketTask?
    private var size: (cols: Int, rows: Int) = (80, 24)

    init(config: RemoteConfig) {
        self.config = config
    }

    func connect() {
        guard let base = config.url, !config.token.isEmpty else {
            state = .disconnected("Falta la configuración: recompila con moca/ios/build.sh")
            return
        }
        task?.cancel(with: .goingAway, reason: nil)

        var components = URLComponents(url: base, resolvingAgainstBaseURL: false)
        components?.queryItems = [
            URLQueryItem(name: "num_rows", value: String(size.rows)),
            URLQueryItem(name: "num_cols", value: String(size.cols)),
            URLQueryItem(name: "token", value: config.token),
        ]
        guard let url = components?.url else {
            state = .disconnected("URL del proxy inválida")
            return
        }

        state = .connecting
        let task = URLSession.shared.webSocketTask(with: url)
        self.task = task
        task.resume()
        state = .connected
        receive(on: task)
    }

    func send(_ bytes: ArraySlice<UInt8>) {
        guard let task, state == .connected else { return }
        task.send(.data(Data(bytes))) { [weak self] error in
            if let error {
                Task { @MainActor in self?.fail(task, error) }
            }
        }
    }

    func resize(cols: Int, rows: Int) {
        size = (cols, rows)
        guard let task, state == .connected else { return }
        let json = #"{"width":\#(cols),"height":\#(rows),"width_px":0,"height_px":0}"#
        task.send(.string(json)) { _ in }
    }

    private func receive(on task: URLSessionWebSocketTask) {
        task.receive { [weak self] result in
            Task { @MainActor in
                guard let self, self.task === task else { return }
                switch result {
                case .success(.data(let data)):
                    self.onOutput?([UInt8](data))
                    self.receive(on: task)
                case .success(.string):
                    self.receive(on: task)
                case .success:
                    self.receive(on: task)
                case .failure(let error):
                    self.fail(task, error)
                }
            }
        }
    }

    private func fail(_ task: URLSessionWebSocketTask, _ error: Error) {
        guard self.task === task else { return }
        self.task = nil
        state = .disconnected("Sin conexión con la XPS (¿Tailscale activo?)")
        _ = error
    }
}
