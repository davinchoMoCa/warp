import SwiftUI

@main
struct MocaWarpApp: App {
    var body: some Scene {
        WindowGroup {
            ContentView()
                .preferredColorScheme(.dark)
        }
    }
}

struct ContentView: View {
    @StateObject private var session = RemoteSession(config: .load())

    var body: some View {
        ZStack(alignment: .top) {
            TerminalContainer(session: session)
                .ignoresSafeArea(.container, edges: .bottom)
            if session.state != .connected {
                StatusBanner(session: session)
            }
        }
        .background(Color.black)
    }
}

private struct StatusBanner: View {
    @ObservedObject var session: RemoteSession

    var body: some View {
        HStack(spacing: 12) {
            Text(session.state.message)
                .font(.callout)
                .foregroundStyle(.white)
            if session.state.canRetry {
                Button("Reconectar") { session.connect() }
                    .buttonStyle(.borderedProminent)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .background(.ultraThinMaterial, in: Capsule())
        .padding(.top, 12)
    }
}
