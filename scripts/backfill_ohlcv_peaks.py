#!/usr/bin/env python3
"""
DMNE QUANT // RIGHT-CENSORED PEAK FORENSICS & OHLCV AUDITOR
Identifies historical winners in v0.7.0 whose peak price was right-censored due to
premature exit rules (e.g. Breakeven Guard (+3%), premature Stagnation TTL, early trails).
Queries DexScreener/GeckoTerminal public APIs where available and summarizes censoring severity.
"""

import csv
import json
import sys
import time
import urllib.request
import urllib.error

ARCHIVE_CSV = "logs/trades_forensic_v070_archive.csv"
OUTPUT_REPORT_JSON = "logs/right_censored_winners_report.json"

def fetch_dexscreener_pair(pair_address):
    url = f"https://api.dexscreener.com/latest/dex/pairs/solana/{pair_address}"
    try:
        req = urllib.request.Request(
            url,
            headers={"User-Agent": "Mozilla/5.0 (compatible; DMNEQuant/0.7.1)"}
        )
        with urllib.request.urlopen(req, timeout=3.0) as resp:
            if resp.status == 200:
                data = json.loads(resp.read().decode())
                pairs = data.get("pairs")
                if pairs and len(pairs) > 0:
                    return pairs[0]
    except Exception as e:
        pass
    return None

def main():
    print("=" * 80)
    print(" DMNE QUANT // RIGHT-CENSORED PEAK FORENSICS (v0.7.0 WINNER AUTOPSY)")
    print("=" * 80)

    try:
        with open(ARCHIVE_CSV, "r", encoding="utf-8") as f:
            trades = list(csv.DictReader(f))
    except Exception as e:
        print(f"[ERROR] Failed to read {ARCHIVE_CSV}: {e}")
        sys.exit(1)

    total_trades = len(trades)
    winners = [t for t in trades if t.get("outcome") == "WIN"]
    total_winners = len(winners)

    print(f"[LOAD] Total archived trades: {total_trades}")
    print(f"[LOAD] Total recorded winners: {total_winners}")

    censored_winners = []
    breakeven_choked = []
    stagnation_choked = []
    short_hold_choked = []

    for t in winners:
        symbol = t.get("token_symbol", "UNKNOWN")
        mint = t.get("token_mint", "")
        pair = t.get("pair_address", "")
        exit_rule = t.get("exit_rule", "")
        hold_secs = int(t.get("hold_duration_secs", "0"))
        realized_pnl_pct = float(t.get("realized_pnl_pct", "0.0"))
        mfe_pnl_pct = float(t.get("mfe_pnl_pct", "0.0"))
        entry_price = float(t.get("entry_price_sol", "0.0"))
        peak_price = float(t.get("peak_price_sol", "0.0"))

        is_censored = False
        reasons = []

        # 1. Breakeven Guard premature choking (capped at +3% to +5% when MFE was climbing)
        if "Breakeven Guard" in exit_rule:
            is_censored = True
            reasons.append("BREAKEVEN_GUARD_PREMATURE_CUTOFF")
            breakeven_choked.append(t)

        # 2. Stagnation TTL premature cutoff on positive momentum
        elif "Stagnation TTL" in exit_rule and realized_pnl_pct > 0.0:
            is_censored = True
            reasons.append("STAGNATION_TTL_MOMENTUM_CUTOFF")
            stagnation_choked.append(t)

        # 3. Short hold duration (< 45s or < 120s with positive early exit)
        elif hold_secs < 45 or ("Early Trail" in exit_rule and hold_secs < 120):
            is_censored = True
            reasons.append("SHORT_HOLD_DURATION_CUTOFF")
            short_hold_choked.append(t)

        if is_censored:
            censored_winners.append({
                "symbol": symbol,
                "mint": mint,
                "pair_address": pair,
                "hold_secs": hold_secs,
                "exit_rule": exit_rule,
                "realized_pnl_pct": realized_pnl_pct,
                "recorded_peak_pnl_pct": mfe_pnl_pct,
                "reasons": reasons,
            })

    print(f"\n[ANALYSIS] Out of {total_winners} total historical winners:")
    print(f"  - Breakeven Guard Choked (+3%..+5% floor bug): {len(breakeven_choked)} trades ({len(breakeven_choked)/total_winners*100:.1f}%)")
    print(f"  - Stagnation TTL Cutoff (positive early exits): {len(stagnation_choked)} trades ({len(stagnation_choked)/total_winners*100:.1f}%)")
    print(f"  - Premature Trail / Short Hold Cutoffs: {len(short_hold_choked)} trades ({len(short_hold_choked)/total_winners*100:.1f}%)")
    print(f"  - TOTAL RIGHT-CENSORED WINNERS: {len(censored_winners)} / {total_winners} ({len(censored_winners)/total_winners*100:.1f}%)")

    # Sample query to DexScreener for available pairs
    print("\n[SAMPLE CHECK] Sampling first 5 right-censored tokens for external market liquidity/pricing:")
    for item in censored_winners[:5]:
        pair_info = fetch_dexscreener_pair(item["pair_address"]) if item["pair_address"] else None
        curr_fdv = pair_info.get("fdv", "N/A") if pair_info else "N/A"
        curr_price_usd = pair_info.get("priceUsd", "N/A") if pair_info else "N/A"
        print(f"  * {item['symbol']} ({item['exit_rule']}): Hold={item['hold_secs']}s, Realized=+{item['realized_pnl_pct']:.1f}%, Peak=+{item['recorded_peak_pnl_pct']:.1f}% | Current FDV: {curr_fdv}")
        time.sleep(0.2)

    report_payload = {
        "total_archived_trades": total_trades,
        "total_winners": total_winners,
        "total_right_censored_winners": len(censored_winners),
        "censored_ratio_pct": round(len(censored_winners) / total_winners * 100.0, 2),
        "breakeven_choked_count": len(breakeven_choked),
        "stagnation_choked_count": len(stagnation_choked),
        "censored_winners": censored_winners,
    }

    with open(OUTPUT_REPORT_JSON, "w", encoding="utf-8") as out:
        json.dump(report_payload, out, indent=2)

    print(f"\n[OK] Detailed Right-Censoring Audit saved to: {OUTPUT_REPORT_JSON}")

if __name__ == "__main__":
    main()
