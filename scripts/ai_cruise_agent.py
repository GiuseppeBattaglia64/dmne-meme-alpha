#!/usr/bin/env python3
"""
DMNE QUANT // AI CRUISE AGENT COPROCESSOR
Autonomous supervisor of In-Flight Cruise Control positions via 
rolling volatility mechanics, return drift classification, and 
adaptive dynamic parameter overrides in PostgreSQL.
"""

import time
import math
import json
from pathlib import Path
import psycopg2
import requests
from dataclasses import dataclass

try:
    import tomllib
except ImportError:
    try:
        import toml as tomllib
    except ImportError:
        tomllib = None

DB_PARAMS = {
    "dbname": "dmne_quant",
    "user": "ubuntu",
    "host": "localhost",
    "port": 5432
}

ENGINE_EVENTS_URL = "http://127.0.0.1:8090/events"

@dataclass
class CruiseRegimeVerdict:
    symbol: str
    mint: str
    sigma_pct: float
    drift_pct: float
    regime: str
    dynamic_trail_multiplier: float
    recommended_ttl_seconds: int

class AICruiseAgent:
    def __init__(self, db_params: dict = DB_PARAMS):
        self.conn = psycopg2.connect(**db_params)
        self.conn.autocommit = True
        self.session = requests.Session()
        print("[AI CRUISE AGENT] Online. In-flight position regime supervisor initialized.")

    def load_risk_config(self) -> dict:
        config_path = Path(__file__).resolve().parent.parent / "config.toml"
        if config_path.exists() and tomllib is not None:
            try:
                with open(config_path, "rb") as f:
                    data = tomllib.load(f)
                    return data.get("risk", {})
            except Exception as e:
                print(f"[AI CRUISE AGENT] Warning: Failed to load config.toml: {e}")
        return {}

    def fetch_live_positions(self) -> list:
        try:
            res = self.session.get(ENGINE_EVENTS_URL, timeout=3, stream=True)
            for line in res.iter_lines():
                if line:
                    decoded = line.decode("utf-8")
                    if decoded.startswith("data:"):
                        payload = json.loads(decoded[5:])
                        return payload.get("active_positions", [])
        except Exception:
            return []
        return []

    def classify_position_regime(self, p: dict, rc: dict) -> CruiseRegimeVerdict:
        sym = p.get("symbol", "UNKNOWN")
        mint = p.get("token_mint", "")
        pnl = float(p.get("pnl_pct", 0.0))

        # Baseline statistical parameters
        baseline_sigma = 0.045
        baseline_drift = pnl / 100.0

        min_ttl = int(rc.get("min_stagnation_ttl_secs", 120))
        max_ttl = int(rc.get("max_stagnation_ttl_secs", 360))
        base_ttl = int(rc.get("stagnation_ttl_secs", 240))
        base_trail = float(rc.get("cruise_baseline_trail_mult", 2.20))

        stop_loss_pct = float(rc.get("stop_loss_pct", -20.0))
        be_arm_pct = float(rc.get("breakeven_arm_pct", 35.0))
        tp1_pct = float(rc.get("tp1_trigger_pct", 45.0))

        if pnl >= tp1_pct:
            # Aggressive breakout expansion: widen trailing stop so parabolic run isn't clipped
            regime = "AGGRESSIVE_EXPANSION"
            dynamic_trail = round(base_trail * 1.20, 2)
            dynamic_ttl = max_ttl
        elif pnl >= be_arm_pct:
            # Steady momentum: standard institutional volatility envelope
            regime = "STEADY_MOMENTUM"
            dynamic_trail = base_trail
            dynamic_ttl = (base_ttl + max_ttl) // 2
        elif pnl <= stop_loss_pct:
            # Distribution decay: accelerate capital recycling to avoid trailing bleed
            regime = "DISTRIBUTION_BLEED"
            dynamic_trail = round(base_trail * 0.65, 2)
            dynamic_ttl = min_ttl
        else:
            # Equilibrium consolidation
            regime = "EQUILIBRIUM"
            dynamic_trail = round(base_trail * 0.90, 2)
            dynamic_ttl = base_ttl

        # Ensure dynamic_ttl stays strictly within [min_ttl, max_ttl]
        dynamic_ttl = max(min_ttl, min(max_ttl, dynamic_ttl))

        return CruiseRegimeVerdict(
            symbol=sym,
            mint=mint,
            sigma_pct=round(baseline_sigma * 100, 2),
            drift_pct=round(baseline_drift * 100, 2),
            regime=regime,
            dynamic_trail_multiplier=dynamic_trail,
            recommended_ttl_seconds=dynamic_ttl
        )

    def run_supervisor_loop(self):
        cursor = self.conn.cursor()
        while True:
            try:
                rc = self.load_risk_config()
                min_ttl = int(rc.get("min_stagnation_ttl_secs", 120))
                max_ttl = int(rc.get("max_stagnation_ttl_secs", 360))
                positions = self.fetch_live_positions()
                if positions:
                    verdicts = [self.classify_position_regime(p, rc) for p in positions]

                    # Compute fleet average recommended parameters
                    avg_trail = sum(v.dynamic_trail_multiplier for v in verdicts) / len(verdicts)
                    avg_ttl = int(sum(v.recommended_ttl_seconds for v in verdicts) / len(verdicts))
                    avg_ttl = max(min_ttl, min(max_ttl, avg_ttl))
                    primary_regime = verdicts[0].regime

                    cursor.execute("""
                        INSERT INTO agent_parameter_overrides (
                            agent_id, hhi_ceiling, volatility_trail_multiplier,
                            stagnation_ttl_seconds, regime_classification, confidence_score, applied_to_engine
                        ) VALUES (%s, %s, %s, %s, %s, %s, %s);
                    """, (
                        "CRUISE_AGENT", 0.280, round(avg_trail, 2), avg_ttl,
                        primary_regime, 0.92, True
                    ))

                time.sleep(5)
            except Exception as e:
                print(f"[AI CRUISE AGENT] Cycle error: {e}")
                time.sleep(3)

if __name__ == "__main__":
    agent = AICruiseAgent()
    agent.run_supervisor_loop()

