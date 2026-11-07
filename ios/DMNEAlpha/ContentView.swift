import SwiftUI
#if canImport(SafariServices)
import SafariServices
#endif
#if canImport(UIKit)
import UIKit
#endif

// MARK: - Configuration Registry (Zero Magic Numbers)
enum AppConfig {
    // Network & Polling
    static let telemetryPollingNanoseconds: UInt64 = 2_000_000_000 // 2 seconds
    static let fxPollingCycleFrequency: Int = 10                  // Every 10 telemetry polls (20s)
    static let networkTimeoutSeconds: TimeInterval = 6.0
    static let defaultTunnelHost: String = "https://downloaded-most-eden-lung.trycloudflare.com"
    static let apiEndpointPath: String = "/api/state"
    static let coinbaseRatesUrl: String = "https://api.coinbase.com/v2/exchange-rates?currency=SOL"
    static let dexscreenerBaseUrl: String = "https://dexscreener.com/solana/"

    // Execution Defaults (Used only before first live state decode)
    static let defaultSlotCount: Int = 4
    static let defaultStopLossPct: Double = -20.0
    static let defaultBreakevenArmPct: Double = 24.0
    static let defaultTp1TriggerPct: Double = 45.0
    static let defaultSolPriceUsd: Double = 188.0
    static let defaultSolPriceEur: Double = 174.0

    // Visual & Layout Constants
    static let cardCornerRadius: CGFloat = 16.0
    static let innerCornerRadius: CGFloat = 12.0
    static let cardBorderOpacity: Double = 0.08
    static let secondaryTextOpacity: Double = 0.70
    static let backgroundGlassOpacity: Double = 0.03
    static let sparklinePointsMinimum: Int = 2
    static let sparklineLineWidth: CGFloat = 2.0
    static let compactLayoutBreakpoint: CGFloat = 720.0
    static let addressTruncatePrefixLength: Int = 4
    static let addressTruncateSuffixLength: Int = 4
    static let largeCountMillionDivisor: Double = 1_000_000.0
    static let largeCountThousandDivisor: Double = 1_000.0
}

// MARK: - Native Haptic Engine
@MainActor
enum HapticFeedback {
    static func notifySuccess() {
        #if canImport(UIKit)
        let generator = UINotificationFeedbackGenerator()
        generator.prepare()
        generator.notificationOccurred(.success)
        #endif
    }

    static func notifyWarning() {
        #if canImport(UIKit)
        let generator = UINotificationFeedbackGenerator()
        generator.prepare()
        generator.notificationOccurred(.warning)
        #endif
    }

    static func notifyError() {
        #if canImport(UIKit)
        let generator = UINotificationFeedbackGenerator()
        generator.prepare()
        generator.notificationOccurred(.error)
        #endif
    }

    static func triggerImpact(_ style: UIImpactFeedbackGenerator.FeedbackStyle = .medium) {
        #if canImport(UIKit)
        let generator = UIImpactFeedbackGenerator(style: style)
        generator.prepare()
        generator.impactOccurred()
        #endif
    }
}

// MARK: - In-App Safari Token Inspector
#if canImport(SafariServices)
struct SafariView: UIViewControllerRepresentable {
    let url: URL

    func makeUIViewController(context: Context) -> SFSafariViewController {
        let configuration = SFSafariViewController.Configuration()
        configuration.entersReaderIfAvailable = false
        configuration.barCollapsingEnabled = true
        return SFSafariViewController(url: url, configuration: configuration)
    }

    func updateUIViewController(_ uiViewController: SFSafariViewController, context: Context) {}
}
#endif

struct InspectedToken: Identifiable, Sendable {
    var id: String { tokenMint }
    let tokenMint: String
    let symbol: String
}

// MARK: - Live OCI Engine Telemetry Models
struct RiskConfig: Codable, Sendable {
    var breakevenArmPct: Double?
    var breakevenFloorPct: Double?
    var stopLossPct: Double?
    var tp1TriggerPct: Double?
    var tp2TriggerPct: Double?
    var maxActiveSlots: Int?
    var kellyFractionNavPct: Double?
    var stagnationTtlSecs: Double?

    enum CodingKeys: String, CodingKey {
        case breakevenArmPct = "breakeven_arm_pct"
        case breakevenFloorPct = "breakeven_floor_pct"
        case stopLossPct = "stop_loss_pct"
        case tp1TriggerPct = "tp1_trigger_pct"
        case tp2TriggerPct = "tp2_trigger_pct"
        case maxActiveSlots = "max_active_slots"
        case kellyFractionNavPct = "kelly_fraction_nav_pct"
        case stagnationTtlSecs = "stagnation_ttl_secs"
    }
}

struct DeskState: Codable, Sendable {
    var currentBankrollSol: Double
    var initialBankrollSol: Double?
    var totalRealizedPnlSol: Double
    var totalTrades: Int
    var wins: Int
    var losses: Int
    var activePositions: [ActivePosition]
    var closedTrades: [SettledTrade]
    var riskConfig: RiskConfig?
    var engineVersion: String?
    var walletPubkey: String?
    var rejectionsCount: Int?
    var floatingPnlSol: Double?

    enum CodingKeys: String, CodingKey {
        case currentBankrollSol = "current_bankroll_sol"
        case initialBankrollSol = "initial_bankroll_sol"
        case totalRealizedPnlSol = "total_realized_pnl_sol"
        case totalTrades = "total_trades"
        case wins, losses
        case activePositions = "active_positions"
        case closedTrades = "closed_trades"
        case riskConfig = "risk_config"
        case engineVersion = "engine_version"
        case walletPubkey = "wallet_pubkey"
        case rejectionsCount = "rejections_count"
        case floatingPnlSol = "floating_pnl_sol"
    }
}

struct ActivePosition: Codable, Identifiable, Sendable {
    var id: String { tokenMint }
    let tokenMint: String
    let symbol: String
    let entryPriceSol: Double?
    let currentPriceSol: Double?
    let peakPriceSol: Double?
    let pnlPct: Double?
    let sizeSol: Double?

    enum CodingKeys: String, CodingKey {
        case tokenMint = "token_mint"
        case symbol
        case entryPriceSol = "entry_price_sol"
        case currentPriceSol = "current_price_sol"
        case peakPriceSol = "peak_price_sol"
        case pnlPct = "pnl_pct"
        case sizeSol = "size_sol"
    }
}

struct SettledTrade: Codable, Identifiable, Sendable {
    var id: String { "\(tokenMint)_\(closedAtEpoch)" }
    let tokenMint: String
    let symbol: String
    let sizeSol: Double
    let pnlSol: Double
    let pnlPct: Double
    let reason: String
    let closedAtEpoch: Double
    let isToxic: Bool?

    enum CodingKeys: String, CodingKey {
        case tokenMint = "token_mint"
        case symbol
        case sizeSol = "size_sol"
        case pnlSol = "pnl_sol"
        case pnlPct = "pnl_pct"
        case reason
        case closedAtEpoch = "closed_at_epoch"
        case isToxic = "is_toxic"
    }
}

struct CoinbaseRatesResponse: Codable {
    struct DataContainer: Codable {
        let currency: String
        let rates: [String: String]
    }
    let data: DataContainer
}

// MARK: - Mathematical Quant Analytics Engine
struct QuantAnalytics: Sendable {
    let profitFactor: Double
    let expectancySol: Double
    let payoffRatio: Double
    let maxDrawdownPct: Double
    let equitySeries: [Double]

    static func calculate(initialBankroll: Double, trades: [SettledTrade]) -> QuantAnalytics {
        var cumulativeEquity = initialBankroll
        var equityPoints: [Double] = [cumulativeEquity]
        var peakEquity = cumulativeEquity
        var maximumDrawdown = 0.0

        var grossWinsSol = 0.0
        var grossLossesSol = 0.0
        var winCount = 0
        var lossCount = 0
        var sumWinPct = 0.0
        var sumLossPct = 0.0

        for trade in trades {
            cumulativeEquity += trade.pnlSol
            equityPoints.append(cumulativeEquity)

            if cumulativeEquity > peakEquity {
                peakEquity = cumulativeEquity
            } else if peakEquity > 0.0 {
                let drawdown = ((peakEquity - cumulativeEquity) / peakEquity) * 100.0
                if drawdown > maximumDrawdown {
                    maximumDrawdown = drawdown
                }
            }

            if trade.pnlSol > 0.0 {
                grossWinsSol += trade.pnlSol
                sumWinPct += trade.pnlPct
                winCount += 1
            } else if trade.pnlSol < 0.0 {
                grossLossesSol += abs(trade.pnlSol)
                sumLossPct += abs(trade.pnlPct)
                lossCount += 1
            }
        }

        let totalValidTrades = winCount + lossCount
        let profitFactorValue = grossLossesSol > 0.0 ? (grossWinsSol / grossLossesSol) : (grossWinsSol > 0.0 ? 99.99 : 0.0)

        let avgWinSol = winCount > 0 ? (grossWinsSol / Double(winCount)) : 0.0
        let avgLossSol = lossCount > 0 ? (grossLossesSol / Double(lossCount)) : 0.0
        let winProbability = totalValidTrades > 0 ? (Double(winCount) / Double(totalValidTrades)) : 0.0
        let lossProbability = totalValidTrades > 0 ? (Double(lossCount) / Double(totalValidTrades)) : 0.0
        let expectancy = (winProbability * avgWinSol) - (lossProbability * avgLossSol)

        let avgWinPct = winCount > 0 ? (sumWinPct / Double(winCount)) : 0.0
        let avgLossPct = lossCount > 0 ? (sumLossPct / Double(lossCount)) : 0.0
        let payoff = avgLossPct > 0.0 ? (avgWinPct / avgLossPct) : 0.0

        return QuantAnalytics(
            profitFactor: profitFactorValue,
            expectancySol: expectancy,
            payoffRatio: payoff,
            maxDrawdownPct: maximumDrawdown,
            equitySeries: equityPoints
        )
    }
}

// MARK: - View Model
@MainActor
final class DeskViewModel: ObservableObject {
    @Published var state: DeskState = DeskState(
        currentBankrollSol: 0.5603,
        initialBankrollSol: 0.5555,
        totalRealizedPnlSol: 0.0047,
        totalTrades: 59,
        wins: 30,
        losses: 29,
        activePositions: [],
        closedTrades: [],
        riskConfig: nil,
        engineVersion: "0.7.1",
        walletPubkey: "CwBPS16Pz6RonsKmYPJAjZR7RbFpbKwJN6CcvJikj55j",
        rejectionsCount: 378600,
        floatingPnlSol: 0.0
    )
    @Published var isConnected: Bool = false
    @Published var tunnelUrl: String = AppConfig.defaultTunnelHost
    @Published var isConnecting: Bool = false
    @Published var statusMessage: String = "Ready."
    @Published var solPriceUsd: Double = AppConfig.defaultSolPriceUsd
    @Published var solPriceEur: Double = AppConfig.defaultSolPriceEur
    @Published var quantMetrics: QuantAnalytics = QuantAnalytics(
        profitFactor: 0.0,
        expectancySol: 0.0,
        payoffRatio: 0.0,
        maxDrawdownPct: 0.0,
        equitySeries: []
    )

    private var isFirstLoadComplete: Bool = false

    func fetchSolPrices() async {
        guard let url = URL(string: AppConfig.coinbaseRatesUrl) else { return }
        do {
            var request = URLRequest(url: url)
            request.timeoutInterval = AppConfig.networkTimeoutSeconds
            let (data, response) = try await URLSession.shared.data(for: request)
            if let httpRes = response as? HTTPURLResponse, httpRes.statusCode == 200 {
                let decoded = try JSONDecoder().decode(CoinbaseRatesResponse.self, from: data)
                if let usd = decoded.data.rates["USD"], let val = Double(usd) { self.solPriceUsd = val }
                if let eur = decoded.data.rates["EUR"], let val = Double(eur) { self.solPriceEur = val }
            }
        } catch {
            // Retain previous spot rates during network transients
        }
    }

    func fetchState() async {
        var clean = tunnelUrl.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !clean.isEmpty else {
            statusMessage = "Please enter your Cloudflare Tunnel URL."
            return
        }

        if clean.hasPrefix("http://") {
            clean = String(clean.dropFirst("http://".count))
        } else if clean.hasPrefix("https://") {
            clean = String(clean.dropFirst("https://".count))
        }

        if let firstSlash = clean.firstIndex(of: "/") {
            clean = String(clean[..<firstSlash])
        }
        if let colonIndex = clean.firstIndex(of: ":") {
            clean = String(clean[..<colonIndex])
        }

        let fullEndpoint = "https://\(clean)\(AppConfig.apiEndpointPath)"
        guard let url = URL(string: fullEndpoint) else {
            statusMessage = "Invalid URL: \(fullEndpoint)"
            return
        }

        if !isFirstLoadComplete {
            isConnecting = true
            statusMessage = "Connecting to \(fullEndpoint)..."
        }
        defer { isConnecting = false }

        do {
            var request = URLRequest(url: url)
            request.timeoutInterval = AppConfig.networkTimeoutSeconds
            let (data, response) = try await URLSession.shared.data(for: request)
            guard let httpResponse = response as? HTTPURLResponse else {
                statusMessage = "No response from server."
                return
            }

            if httpResponse.statusCode == 200 {
                let decoder = JSONDecoder()
                let newState = try decoder.decode(DeskState.self, from: data)

                // Haptic feedback triggers
                if isFirstLoadComplete {
                    let prevCount = self.state.closedTrades.count
                    let newCount = newState.closedTrades.count
                    if newCount > prevCount, let latest = newState.closedTrades.last {
                        if latest.pnlPct >= 0 {
                            HapticFeedback.notifySuccess()
                        } else {
                            HapticFeedback.notifyError()
                        }
                    } else if newState.activePositions.count > self.state.activePositions.count {
                        HapticFeedback.triggerImpact(.heavy)
                    }
                }

                self.state = newState
                let initial = newState.initialBankrollSol ?? newState.currentBankrollSol
                self.quantMetrics = QuantAnalytics.calculate(initialBankroll: initial, trades: newState.closedTrades)
                self.isConnected = true
                self.isFirstLoadComplete = true
                self.statusMessage = "Connected successfully!"
            } else {
                self.statusMessage = "HTTP Error: \(httpResponse.statusCode)"
                self.isConnected = false
            }
        } catch {
            self.isConnected = false
            self.statusMessage = "Error: \(error.localizedDescription)"
        }
    }
}

// MARK: - Cumulative Equity Sparkline
struct EquitySparklineView: View {
    let series: [Double]
    let lineColor: Color

    var body: some View {
        GeometryReader { geo in
            if series.count >= AppConfig.sparklinePointsMinimum {
                let minValue = series.min() ?? 0.0
                let maxValue = series.max() ?? 1.0
                let range = max(maxValue - minValue, 0.0001)

                let points: [CGPoint] = series.indices.map { idx in
                    let normalizedX = CGFloat(idx) / CGFloat(series.count - 1)
                    let x = normalizedX * geo.size.width
                    let normalizedY = CGFloat((series[idx] - minValue) / range)
                    let y = geo.size.height - (normalizedY * geo.size.height)
                    return CGPoint(x: x, y: y)
                }

                Path { path in
                    path.move(to: points[0])
                    for pt in points.dropFirst() {
                        path.addLine(to: pt)
                    }
                }
                .stroke(
                    LinearGradient(
                        colors: [lineColor.opacity(0.4), lineColor],
                        startPoint: .leading,
                        endPoint: .trailing
                    ),
                    style: StrokeStyle(lineWidth: AppConfig.sparklineLineWidth, lineCap: .round, lineJoin: .round)
                )
                .shadow(color: lineColor.opacity(0.6), radius: 3)
            }
        }
    }
}

// MARK: - Main Interface
struct ContentView: View {
    @StateObject private var vm = DeskViewModel()
    @State private var showSettings: Bool = false
    @State private var inspectedToken: InspectedToken? = nil

    var body: some View {
        ZStack {
            Color(red: 0.03, green: 0.04, blue: 0.06).ignoresSafeArea()

            RadialGradient(
                colors: [Color.cyan.opacity(0.12), Color.clear],
                center: .topLeading,
                startRadius: 40,
                endRadius: 500
            ).ignoresSafeArea()

            VStack(spacing: 12) {
                // Top Header Bar
                HStack(alignment: .center) {
                    HStack(spacing: 8) {
                        Circle()
                            .fill(vm.isConnected ? Color.green : Color.orange)
                            .frame(width: 10, height: 10)
                            .shadow(color: vm.isConnected ? .green : .orange, radius: 5)
                        Text("DMNE ALPHA")
                            .font(.system(.title3, design: .monospaced, weight: .bold))
                            .foregroundColor(.white)
                    }

                    Spacer()

                    // Live Spot Ticker
                    HStack(spacing: 6) {
                        Text("SOL")
                            .font(.system(size: 10, weight: .bold, design: .monospaced))
                            .foregroundColor(.secondary)
                        Text(String(format: "$%.1f", vm.solPriceUsd))
                            .font(.system(size: 11, weight: .bold, design: .monospaced))
                            .foregroundColor(.white)
                        Text("•")
                            .foregroundColor(.secondary.opacity(0.4))
                        Text(String(format: "€%.1f", vm.solPriceEur))
                            .font(.system(size: 11, weight: .bold, design: .monospaced))
                            .foregroundColor(.cyan)
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 5)
                    .background(Color.white.opacity(AppConfig.backgroundGlassOpacity))
                    .clipShape(Capsule())
                    .overlay(Capsule().stroke(Color.white.opacity(AppConfig.cardBorderOpacity), lineWidth: 1))

                    // OCI State Action
                    Button {
                        showSettings.toggle()
                    } label: {
                        HStack(spacing: 6) {
                            Image(systemName: "antenna.radiowaves.left.and.right")
                            Text(vm.isConnected ? "LIVE OCI" : "CONNECT OCI")
                                .font(.system(size: 11, weight: .semibold, design: .monospaced))
                        }
                        .padding(.horizontal, 12)
                        .padding(.vertical, 6)
                        .background(.ultraThinMaterial)
                        .clipShape(Capsule())
                        .overlay(Capsule().stroke(Color.white.opacity(0.12), lineWidth: 1))
                        .foregroundColor(vm.isConnected ? .green : .cyan)
                    }
                }
                .padding(.horizontal)

                // Daemon Telemetry Status Strip
                HStack(spacing: 12) {
                    Label("v\(vm.state.engineVersion ?? "0.7.1")", systemImage: "cpu")
                    Text("•")
                    Label("\(formatRejectionCount(vm.state.rejectionsCount ?? 0)) scanned", systemImage: "shield.checkerboard")
                    Text("•")
                    Label(formatWalletAddress(vm.state.walletPubkey ?? ""), systemImage: "wallet.pass")
                    Spacer()
                }
                .font(.system(size: 10, design: .monospaced))
                .foregroundColor(.secondary.opacity(AppConfig.secondaryTextOpacity))
                .padding(.horizontal)

                // High-Level Portfolio & Quant Analytics Row
                HStack(spacing: 10) {
                    // Portfolio NAV with Embedded Sparkline
                    VStack(alignment: .leading, spacing: 4) {
                        Text("PORTFOLIO NAV")
                            .font(.system(size: 9, weight: .bold, design: .monospaced))
                            .foregroundColor(.secondary)
                        Text(String(format: "%.4f SOL", vm.state.currentBankrollSol))
                            .font(.system(size: 19, weight: .bold, design: .monospaced))
                            .foregroundColor(.white)
                        Text(String(format: "$%.2f • €%.2f", vm.state.currentBankrollSol * vm.solPriceUsd, vm.state.currentBankrollSol * vm.solPriceEur))
                            .font(.system(size: 10, weight: .semibold, design: .monospaced))
                            .foregroundColor(.white.opacity(AppConfig.secondaryTextOpacity))

                        // Running Equity Sparkline
                        EquitySparklineView(
                            series: vm.quantMetrics.equitySeries,
                            lineColor: .cyan
                        )
                        .frame(height: 26)
                        .padding(.top, 2)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(12)
                    .background(.ultraThinMaterial)
                    .clipShape(RoundedRectangle(cornerRadius: AppConfig.cardCornerRadius))
                    .overlay(RoundedRectangle(cornerRadius: AppConfig.cardCornerRadius).stroke(Color.white.opacity(AppConfig.cardBorderOpacity), lineWidth: 1))

                    // Net Realized Card
                    let pnlUsd = vm.state.totalRealizedPnlSol * vm.solPriceUsd
                    let pnlEur = vm.state.totalRealizedPnlSol * vm.solPriceEur
                    VStack(alignment: .leading, spacing: 4) {
                        Text("NET REALIZED")
                            .font(.system(size: 9, weight: .bold, design: .monospaced))
                            .foregroundColor(.secondary)
                        Text(String(format: "%@%.4f SOL", vm.state.totalRealizedPnlSol >= 0 ? "+" : "", vm.state.totalRealizedPnlSol))
                            .font(.system(size: 19, weight: .bold, design: .monospaced))
                            .foregroundColor(vm.state.totalRealizedPnlSol >= 0 ? .green : .pink)
                        Text(String(format: "%@$%.2f • %@€%.2f", pnlUsd >= 0 ? "+" : "", pnlUsd, pnlEur >= 0 ? "+" : "", pnlEur))
                            .font(.system(size: 10, weight: .semibold, design: .monospaced))
                            .foregroundColor(.white.opacity(AppConfig.secondaryTextOpacity))
                        Text("\(vm.state.wins)W / \(vm.state.losses)L (\(String(format: "%.1f", Double(vm.state.wins) / Double(max(1, vm.state.totalTrades)) * 100))%)")
                            .font(.system(size: 10, weight: .medium, design: .monospaced))
                            .foregroundColor(vm.state.totalRealizedPnlSol >= 0 ? .green : .pink)
                            .padding(.top, 4)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(12)
                    .background(.ultraThinMaterial)
                    .clipShape(RoundedRectangle(cornerRadius: AppConfig.cardCornerRadius))
                    .overlay(RoundedRectangle(cornerRadius: AppConfig.cardCornerRadius).stroke(Color.white.opacity(AppConfig.cardBorderOpacity), lineWidth: 1))

                    // Institutional Quant Edge Card
                    VStack(alignment: .leading, spacing: 5) {
                        Text("STATISTICAL EDGE")
                            .font(.system(size: 9, weight: .bold, design: .monospaced))
                            .foregroundColor(.secondary)

                        HStack {
                            VStack(alignment: .leading, spacing: 1) {
                                Text("PROFIT FACTOR")
                                    .font(.system(size: 8, design: .monospaced))
                                    .foregroundColor(.secondary)
                                Text(String(format: "%.2f", vm.quantMetrics.profitFactor))
                                    .font(.system(size: 13, weight: .bold, design: .monospaced))
                                    .foregroundColor(vm.quantMetrics.profitFactor >= 1.25 ? .green : (vm.quantMetrics.profitFactor >= 1.0 ? .yellow : .pink))
                            }
                            Spacer()
                            VStack(alignment: .trailing, spacing: 1) {
                                Text("MAX DD")
                                    .font(.system(size: 8, design: .monospaced))
                                    .foregroundColor(.secondary)
                                Text(String(format: "-%.1f%%", vm.quantMetrics.maxDrawdownPct))
                                    .font(.system(size: 13, weight: .bold, design: .monospaced))
                                    .foregroundColor(.pink)
                            }
                        }

                        HStack {
                            VStack(alignment: .leading, spacing: 1) {
                                Text("EXPECTANCY")
                                    .font(.system(size: 8, design: .monospaced))
                                    .foregroundColor(.secondary)
                                Text(String(format: "%@%.5f SOL", vm.quantMetrics.expectancySol >= 0 ? "+" : "", vm.quantMetrics.expectancySol))
                                    .font(.system(size: 11, weight: .bold, design: .monospaced))
                                    .foregroundColor(vm.quantMetrics.expectancySol >= 0 ? .green : .pink)
                            }
                            Spacer()
                            VStack(alignment: .trailing, spacing: 1) {
                                Text("PAYOFF")
                                    .font(.system(size: 8, design: .monospaced))
                                    .foregroundColor(.secondary)
                                Text(String(format: "%.2fx", vm.quantMetrics.payoffRatio))
                                    .font(.system(size: 11, weight: .bold, design: .monospaced))
                                    .foregroundColor(.white)
                            }
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(12)
                    .background(.ultraThinMaterial)
                    .clipShape(RoundedRectangle(cornerRadius: AppConfig.cardCornerRadius))
                    .overlay(RoundedRectangle(cornerRadius: AppConfig.cardCornerRadius).stroke(Color.white.opacity(AppConfig.cardBorderOpacity), lineWidth: 1))
                }
                .padding(.horizontal)

                // Main Execution Workspace
                GeometryReader { geo in
                    let maxSlots = vm.state.riskConfig?.maxActiveSlots ?? AppConfig.defaultSlotCount
                    if geo.size.width > AppConfig.compactLayoutBreakpoint {
                        HStack(alignment: .top, spacing: 14) {
                            // Left Column: Active Slots
                            VStack(alignment: .leading, spacing: 10) {
                                Text("EXECUTION DESK (\(maxSlots) SLOTS)")
                                    .font(.system(size: 11, weight: .bold, design: .monospaced))
                                    .foregroundColor(.secondary)

                                LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 10) {
                                    ForEach(0..<maxSlots, id: \.self) { index in
                                        ExecutionSlotCard(
                                            slotIndex: index,
                                            position: vm.state.activePositions.indices.contains(index) ? vm.state.activePositions[index] : nil,
                                            riskConfig: vm.state.riskConfig,
                                            solPriceUsd: vm.solPriceUsd,
                                            onInspect: { mint, sym in
                                                self.inspectedToken = InspectedToken(tokenMint: mint, symbol: sym)
                                            }
                                        )
                                    }
                                }
                            }
                            .frame(width: geo.size.width * 0.52)

                            // Right Column: Settled Trades Forensic Feed
                            VStack(alignment: .leading, spacing: 10) {
                                HStack {
                                    Text("FORENSIC SETTLED TRADES (\(vm.state.closedTrades.count))")
                                        .font(.system(size: 11, weight: .bold, design: .monospaced))
                                        .foregroundColor(.secondary)
                                    Spacer()
                                    Text("Tap row to inspect")
                                        .font(.system(size: 9, design: .monospaced))
                                        .foregroundColor(.secondary.opacity(0.6))
                                }

                                SettledTradesList(
                                    trades: vm.state.closedTrades.reversed(),
                                    solPriceUsd: vm.solPriceUsd,
                                    onSelect: { trade in
                                        self.inspectedToken = InspectedToken(tokenMint: trade.tokenMint, symbol: trade.symbol)
                                    }
                                )
                            }
                            .frame(width: geo.size.width * 0.45)
                        }
                        .padding(.horizontal)
                    } else {
                        ScrollView {
                            VStack(spacing: 14) {
                                LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 10) {
                                    ForEach(0..<maxSlots, id: \.self) { index in
                                        ExecutionSlotCard(
                                            slotIndex: index,
                                            position: vm.state.activePositions.indices.contains(index) ? vm.state.activePositions[index] : nil,
                                            riskConfig: vm.state.riskConfig,
                                            solPriceUsd: vm.solPriceUsd,
                                            onInspect: { mint, sym in
                                                self.inspectedToken = InspectedToken(tokenMint: mint, symbol: sym)
                                            }
                                        )
                                    }
                                }
                                SettledTradesList(
                                    trades: vm.state.closedTrades.reversed(),
                                    solPriceUsd: vm.solPriceUsd,
                                    onSelect: { trade in
                                        self.inspectedToken = InspectedToken(tokenMint: trade.tokenMint, symbol: trade.symbol)
                                    }
                                )
                            }
                            .padding(.horizontal)
                        }
                    }
                }
            }
            .padding(.top, 8)
        }
        .preferredColorScheme(.dark)
        .sheet(isPresented: $showSettings) {
            HostSettingsView(vm: vm)
        }
        .sheet(item: $inspectedToken) { token in
            #if canImport(SafariServices)
            if let targetUrl = URL(string: "\(AppConfig.dexscreenerBaseUrl)\(token.tokenMint)") {
                SafariView(url: targetUrl)
                    .ignoresSafeArea()
            }
            #else
            Text("Inspecting \(token.symbol)")
            #endif
        }
        .task {
            var cycleCount = 0
            while !Task.isCancelled {
                await vm.fetchState()
                if cycleCount % AppConfig.fxPollingCycleFrequency == 0 {
                    await vm.fetchSolPrices()
                }
                cycleCount += 1
                try? await Task.sleep(nanoseconds: AppConfig.telemetryPollingNanoseconds)
            }
        }
    }

    private func formatWalletAddress(_ pubkey: String) -> String {
        let minLength = AppConfig.addressTruncatePrefixLength + AppConfig.addressTruncateSuffixLength
        guard pubkey.count >= minLength else { return pubkey }
        return "\(pubkey.prefix(AppConfig.addressTruncatePrefixLength))...\(pubkey.suffix(AppConfig.addressTruncateSuffixLength))"
    }

    private func formatRejectionCount(_ count: Int) -> String {
        if count >= Int(AppConfig.largeCountMillionDivisor) {
            return String(format: "%.1fM", Double(count) / AppConfig.largeCountMillionDivisor)
        } else if count >= Int(AppConfig.largeCountThousandDivisor) {
            return String(format: "%.1fk", Double(count) / AppConfig.largeCountThousandDivisor)
        }
        return "\(count)"
    }
}

// MARK: - Active Slot Card with Execution Targets
struct ExecutionSlotCard: View {
    let slotIndex: Int
    let position: ActivePosition?
    let riskConfig: RiskConfig?
    let solPriceUsd: Double
    let onInspect: (String, String) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text("SLOT \(slotIndex + 1)")
                    .font(.system(size: 9, weight: .bold, design: .monospaced))
                    .foregroundColor(.secondary)
                Spacer()
                Text(position == nil ? "READY" : "IN POSITION")
                    .font(.system(size: 8, weight: .bold, design: .monospaced))
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(position == nil ? Color.green.opacity(0.15) : Color.cyan.opacity(0.2))
                    .foregroundColor(position == nil ? .green : .cyan)
                    .clipShape(Capsule())
            }

            if let pos = position {
                HStack {
                    Button {
                        onInspect(pos.tokenMint, pos.symbol)
                    } label: {
                        HStack(spacing: 4) {
                            Text(pos.symbol)
                                .font(.system(size: 16, weight: .bold, design: .monospaced))
                                .foregroundColor(.white)
                            Image(systemName: "arrow.up.right.square")
                                .font(.system(size: 10))
                                .foregroundColor(.cyan)
                        }
                    }
                    Spacer()
                    if let size = pos.sizeSol {
                        Text(String(format: "$%.1f", size * solPriceUsd))
                            .font(.system(size: 10, design: .monospaced))
                            .foregroundColor(.secondary)
                    }
                }

                if let pnl = pos.pnlPct {
                    HStack {
                        Text(String(format: "%@%.2f%%", pnl >= 0 ? "+" : "", pnl))
                            .font(.system(size: 13, weight: .bold, design: .monospaced))
                            .foregroundColor(pnl >= 0 ? .green : .pink)
                        Spacer()
                    }

                    // Execution Target Gauge
                    ExecutionTargetGauge(currentPnlPct: pnl, config: riskConfig)
                        .padding(.top, 2)
                }
            } else {
                Spacer()
                VStack(spacing: 3) {
                    Image(systemName: "waveform.path.ecg")
                        .font(.system(size: 18))
                        .foregroundColor(.secondary.opacity(0.3))
                    Text("Awaiting Signal")
                        .font(.system(size: 9, design: .monospaced))
                        .foregroundColor(.secondary.opacity(0.5))
                }
                .frame(maxWidth: .infinity)
                Spacer()
            }
        }
        .frame(height: 118)
        .padding(10)
        .background(Color.white.opacity(AppConfig.backgroundGlassOpacity))
        .clipShape(RoundedRectangle(cornerRadius: AppConfig.innerCornerRadius))
        .overlay(RoundedRectangle(cornerRadius: AppConfig.innerCornerRadius).stroke(Color.white.opacity(AppConfig.cardBorderOpacity), lineWidth: 1))
    }
}

// MARK: - Visual Target Gauge (SL -> BE -> TP1)
struct ExecutionTargetGauge: View {
    let currentPnlPct: Double
    let config: RiskConfig?

    var body: some View {
        let stopLoss = config?.stopLossPct ?? AppConfig.defaultStopLossPct
        let tp1 = config?.tp1TriggerPct ?? AppConfig.defaultTp1TriggerPct
        let totalSpan = max(tp1 - stopLoss, 1.0)
        let normalized = min(max((currentPnlPct - stopLoss) / totalSpan, 0.0), 1.0)

        VStack(spacing: 2) {
            GeometryReader { geo in
                ZStack(alignment: .leading) {
                    Capsule()
                        .fill(Color.white.opacity(0.08))
                        .frame(height: 4)

                    // Target indicator bar
                    Capsule()
                        .fill(currentPnlPct >= 0 ? Color.green : Color.pink)
                        .frame(width: max(geo.size.width * CGFloat(normalized), 4), height: 4)
                }
            }
            .frame(height: 4)

            HStack {
                Text(String(format: "SL %.0f%%", stopLoss))
                Spacer()
                Text(String(format: "BE +%.0f%%", config?.breakevenArmPct ?? AppConfig.defaultBreakevenArmPct))
                Spacer()
                Text(String(format: "TP1 +%.0f%%", tp1))
            }
            .font(.system(size: 7, design: .monospaced))
            .foregroundColor(.secondary.opacity(0.6))
        }
    }
}

// MARK: - Settled Trades List
struct SettledTradesList: View {
    let trades: [SettledTrade]
    let solPriceUsd: Double
    let onSelect: (SettledTrade) -> Void

    var body: some View {
        ScrollView {
            LazyVStack(spacing: 6) {
                ForEach(trades) { trade in
                    Button {
                        onSelect(trade)
                    } label: {
                        HStack {
                            VStack(alignment: .leading, spacing: 2) {
                                HStack(spacing: 4) {
                                    Text(trade.symbol)
                                        .font(.system(size: 12, weight: .bold, design: .monospaced))
                                        .foregroundColor(.white)
                                    Image(systemName: "arrow.up.right")
                                        .font(.system(size: 8))
                                        .foregroundColor(.cyan.opacity(0.7))
                                }
                                Text(trade.reason)
                                    .font(.system(size: 9, design: .monospaced))
                                    .foregroundColor(.secondary)
                            }
                            Spacer()
                            VStack(alignment: .trailing, spacing: 2) {
                                Text(String(format: "%@%.2f%%", trade.pnlPct >= 0 ? "+" : "", trade.pnlPct))
                                    .font(.system(size: 12, weight: .bold, design: .monospaced))
                                    .foregroundColor(trade.pnlPct >= 0 ? .green : .pink)

                                let usd = trade.pnlSol * solPriceUsd
                                Text(String(format: "%@%.4f SOL (%@$%.2f)", trade.pnlSol >= 0 ? "+" : "", trade.pnlSol, usd >= 0 ? "+" : "", usd))
                                    .font(.system(size: 9, design: .monospaced))
                                    .foregroundColor(.secondary)
                            }
                        }
                        .padding(9)
                        .background(.ultraThinMaterial)
                        .clipShape(RoundedRectangle(cornerRadius: AppConfig.innerCornerRadius))
                        .overlay(RoundedRectangle(cornerRadius: AppConfig.innerCornerRadius).stroke(Color.white.opacity(0.04), lineWidth: 1))
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }
}

// MARK: - Host Settings Modal
struct HostSettingsView: View {
    @ObservedObject var vm: DeskViewModel
    @Environment(\.dismiss) var dismiss

    var body: some View {
        NavigationStack {
            Form {
                Section("Cloudflare HTTPS Tunnel Host") {
                    TextField("https://xxx.trycloudflare.com", text: $vm.tunnelUrl)
                        .autocorrectionDisabled()
                        .textInputAutocapitalization(.never)
                        .keyboardType(.URL)
                }

                Section("Connection Status") {
                    if vm.isConnecting {
                        HStack(spacing: 10) {
                            ProgressView()
                            Text("Connecting...")
                                .font(.system(size: 13, design: .monospaced))
                        }
                    } else {
                        Text(vm.statusMessage)
                            .font(.system(size: 12, design: .monospaced))
                            .foregroundColor(vm.isConnected ? .green : .orange)
                    }
                }

                Section {
                    Button(action: {
                        Task {
                            await vm.fetchState()
                            if vm.isConnected { dismiss() }
                        }
                    }) {
                        Text(vm.isConnecting ? "Verifying..." : "Connect Live")
                            .font(.headline)
                            .frame(maxWidth: .infinity, alignment: .center)
                            .foregroundColor(.cyan)
                    }
                    .disabled(vm.isConnecting)
                }
            }
            .navigationTitle("OCI Link Configuration")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                Button("Done") { dismiss() }
            }
        }
        .preferredColorScheme(.dark)
    }
}
