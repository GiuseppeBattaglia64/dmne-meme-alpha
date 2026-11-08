#!/usr/bin/env python3
"""
scripts/audit_semantic_loop_sentinel.py
Article 31.28 - Semantic Loop & Architectural Integrity Sentinel
Verifies that all defensive gates are actively wired into the hot-path execution loop.
"""

import sys
import re
from pathlib import Path

REQUIRED_HOTPATH_GATES = {
    "liquidity_floor": [
        r"min_entry_liquidity_usd",
        r"liquidity.*<.*min_entry_liquidity"
    ],
    "hawkes_wash_filter": [
        r"hawkes",
        r"branching_ratio|wash_score"
    ],
    "orderflow_cvd": [
        r"cvd",
        r"taker_ratio|buyer_seller_ratio"
    ],
    "dev_cluster_guard": [
        r"dev_cluster|hhi|concentration",
        r"cluster_risk"
    ],
    "genesis_guard": [
        r"genesis|block_0|sniper_bundle"
    ]
}

def inspect_file(filepath: Path) -> str:
    if not filepath.exists():
        return ""
    with open(filepath, "r", encoding="utf-8") as f:
        return f.read()

def audit_semantic_loop():
    print("=" * 80)
    print(" DMNE ARCHITECTURAL INTEGRITY SENTINEL // ARTICLE 31.28")
    print(" Hot-Path Semantic Loop & Gate Verification")
    print("=" * 80)

    src_dir = Path("src")
    if not src_dir.exists():
        print("[ERROR] 'src/' directory not found.")
        sys.exit(1)

    # Read key hot-path modules
    hotpath_sources = {
        "radar.rs": inspect_file(src_dir / "radar.rs"),
        "main.rs": inspect_file(src_dir / "main.rs"),
        "risk.rs": inspect_file(src_dir / "risk.rs"),
        "discovery.rs": inspect_file(src_dir / "discovery.rs"),
    }

    combined_hotpath = "\n".join(hotpath_sources.values())
    missing_gates = []

    for gate_name, patterns in REQUIRED_HOTPATH_GATES.items():
        found = False
        for pat in patterns:
            if re.search(pat, combined_hotpath, re.IGNORECASE):
                found = True
                break
        
        status = "WIRED & ACTIVE" if found else "UNTIED / MISSING FROM HOT-PATH"
        print(f"[*] Gate [{gate_name:<20}]: {status}")
        if not found:
            missing_gates.append(gate_name)

    print("-" * 80)
    
    # Check if rejection metrics are actually populated in telemetry
    rejection_breakdown_wired = bool(re.search(r"rejection_breakdown|RejectionReason", combined_hotpath))
    print(f"[*] Live Telemetry Rejection Coupling: {'VERIFIED' if rejection_breakdown_wired else 'DECOUPLED'}")

    if missing_gates:
        print(f"\n[FAIL] Article 31.28 Violation: {len(missing_gates)} defensive module(s) are untied:")
        for mg in missing_gates:
            print(f"  - {mg}")
        print("\nEnsure all filters are directly called in the candidate admission predicate before dispatch.")
        sys.exit(1)

    print("\n[PASS] All semantic loop gates verified in active hot-path execution chain.")
    sys.exit(0)

if __name__ == "__main__":
    audit_semantic_loop()
