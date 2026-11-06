import Foundation

struct PositionItem: Identifiable, Codable {
    var id: String { token_mint }
    let symbol: String
    let token_mint: String
    let size_sol: Double
    let pnl_pct: Double
}

struct ServerTelemetry: Codable {
    let current_bankroll_sol: Double?
    let realized_net_pnl: Double?
    let active_positions: [PositionItem]?
}

@MainActor
class DeskViewModel: ObservableObject {
    @Published var bankroll: Double = 0.5000
    @Published var realizedPnl: Double = 0.0000
    @Published var positions: [PositionItem] = []
    @Published var isConnected: Bool = false

    private var streamTask: Task<Void, Never>?
    let serverHost = "130.61.187.215" // Your OCI Milan Server IP

    init() {
        connect()
    }

    func connect() {
        streamTask?.cancel()
        streamTask = Task {
            guard let url = URL(string: "http://\(serverHost):8090/events") else { return }
            do {
                let (bytes, resp) = try await URLSession.shared.bytes(from: url)
                guard (resp as? HTTPURLResponse)?.statusCode == 200 else { return }
                self.isConnected = true

                for try await line in bytes.lines {
                    if line.hasPrefix("data: ") {
                        let jsonStr = String(line.dropFirst(6))
                        if let data = jsonStr.data(using: .utf8),
                           let t = try? JSONDecoder().decode(ServerTelemetry.self, from: data) {
                            if let b = t.current_bankroll_sol { self.bankroll = b }
                            if let pos = t.active_positions { self.positions = pos }
                        }
                    }
                }
            } catch {
                self.isConnected = false
            }
        }
    }
}
