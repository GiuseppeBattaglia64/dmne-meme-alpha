import SwiftUI

struct ContentView: View {
    @EnvironmentObject var desk: DeskViewModel
    @State private var selectedTab = 0

    var body: some View {
        NavigationStack {
            ZStack {
                Color(red: 0.04, green: 0.06, blue: 0.11).ignoresSafeArea()

                VStack(spacing: 0) {
                    // Top Telemetry Header HUD
                    HeaderHUDView()
                        .padding(.horizontal, 16)
                        .padding(.top, 8)
                        .padding(.bottom, 12)

                    // Tab Switcher
                    Picker("", selection: $selectedTab) {
                        Text("Active Desk").tag(0)
                        Text("Radar Stream").tag(1)
                        Text("Ledger").tag(2)
                    }
                    .pickerStyle(.segmented)
                    .padding(.horizontal, 16)
                    .padding(.bottom, 12)

                    // Content View
                    TabView(selection: $selectedTab) {
                        ActivePositionsListView()
                            .tag(0)
                        RadarListView()
                            .tag(1)
                        SettledLedgerView()
                            .tag(2)
                    }
                    .tabViewStyle(.page(indexDisplayMode: .never))
                }
            }
            .navigationTitle("DMNE ALPHA")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .navigationBarTrailing) {
                    Button(action: { desk.connect() }) {
                        Image(systemName: desk.isConnected ? "antenna.radiowaves.left.and.right" : "antenna.radiowaves.left.and.right.slash")
                            .foregroundColor(desk.isConnected ? .green : .red)
                    }
                }
            }
        }
    }
}

struct HeaderHUDView: View {
    @EnvironmentObject var desk: DeskViewModel

    var body: some View {
        HStack {
            VStack(alignment: .leading, spacing: 4) {
                Text("PORTFOLIO NAV")
                    .font(.caption2)
                    .foregroundColor(.secondary)
                Text(String(format: "%.4f SOL", desk.bankroll))
                    .font(.system(.title3, design: .monospaced, weight: .bold))
                    .foregroundColor(.cyan)
            }
            Spacer()
            VStack(alignment: .trailing, spacing: 4) {
                Text("REALIZED PNL")
                    .font(.caption2)
                    .foregroundColor(.secondary)
                Text(String(format: "%+.4f SOL", desk.realizedPnl))
                    .font(.system(.subheadline, design: .monospaced, weight: .bold))
                    .foregroundColor(desk.realizedPnl >= 0 ? .green : .red)
            }
        }
        .padding(14)
        .background(Color(red: 0.08, green: 0.12, blue: 0.20))
        .cornerRadius(10)
        .overlay(
            RoundedRectangle(cornerRadius: 10)
                .stroke(Color.white.opacity(0.08), lineWidth: 1)
        )
    }
}

struct ActivePositionsListView: View {
    @EnvironmentObject var desk: DeskViewModel

    var body: some View {
        ScrollView {
            LazyVStack(spacing: 12) {
                if desk.positions.isEmpty {
                    VStack(spacing: 8) {
                        Text("No Active In-Flight Trades")
                            .font(.subheadline)
                            .foregroundColor(.secondary)
                        Text("Awaiting High-Conviction Queue Pop...")
                            .font(.caption2)
                            .foregroundColor(.gray)
                    }
                    .padding(.top, 40)
                } else {
                    ForEach(desk.positions) { pos in
                        HStack {
                            VStack(alignment: .leading, spacing: 4) {
                                Text(pos.symbol)
                                    .font(.headline.bold())
                                Text(String(format: "Size: %.4f SOL", pos.size_sol))
                                    .font(.caption2)
                                    .foregroundColor(.secondary)
                            }
                            Spacer()
                            Text(String(format: "%+.2f%%", pos.pnl_pct))
                                .font(.system(.title3, design: .monospaced, weight: .bold))
                                .foregroundColor(pos.pnl_pct >= 0 ? .green : .red)
                        }
                        .padding(14)
                        .background(Color(red: 0.08, green: 0.12, blue: 0.20))
                        .cornerRadius(10)
                    }
                }
            }
            .padding(.horizontal, 16)
        }
    }
}

struct RadarListView: View {
    var body: some View {
        Text("Radar Scanning Active (135,000+ Pools)").foregroundColor(.secondary)
    }
}

struct SettledLedgerView: View {
    var body: some View {
        Text("Forensic Trade Ledger").foregroundColor(.secondary)
    }
}
