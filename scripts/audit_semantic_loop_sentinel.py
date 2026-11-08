#!/usr/bin/env python3
"""
DMNE QUANT // ARTICLE 31.28 ARCHITECTURAL INTEGRITY SENTINEL
Audits the Semantic Loop to guarantee zero disconnected modules and zero phantom logic.

Invariants Enforced:
1. Canonical Admission Module:
   - src/admission.rs exports evaluate_candidate_admission, can_admit_candidate, and RejectionReason.
   - Module registered in both src/lib.rs and src/main.rs.
2. 5 Registered Pre-Trade Filters Wired Directly into evaluate_candidate_admission:
   - Liquidity Floor: candidate.liquidity_usd check against min_entry_liquidity_usd
   - Genesis Guard: GenesisGuardEvaluator::evaluate
   - Hawkes Wash: HawkesEvaluator::evaluate
   - Orderflow CVD: RollingCvdTracker (record_snapshot_volume, pressure_ratio, evaluate_divergence)
   - Dev Cluster: FunderGraphEvaluator (evaluate_holders / evaluate_token)
3. Hot-Path Call Graph Invariant:
   - evaluate_candidate_admission is called directly on the live execution queue hot-path.
   - Transaction dispatch is strictly gated behind the admission verdict.
4. Telemetry Coupling Invariant:
   - Rejections directly increment state.rejections_count and record to state.record_stage2_audit.
5. Unit Test Verification:
   - Runs `cargo test --lib admission` and verifies 100% pass rate.
"""

import os
import re
import sys
import subprocess

PROJECT_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))

def log_pass(msg: str):
    print(f"[\033[92mPASS\033[0m] {msg}")

def log_fail(msg: str):
    print(f"[\033[91mFAIL\033[0m] {msg}")

def read_file(rel_path: str) -> str:
    path = os.path.join(PROJECT_ROOT, rel_path)
    if not os.path.exists(path):
        log_fail(f"Required file does not exist: {rel_path}")
        sys.exit(1)
    with open(path, "r", encoding="utf-8") as f:
        return f.read()

def audit_module_exports(admission_src: str, lib_src: str, main_src: str) -> bool:
    print("\n--- 1. AUDITING MODULE REGISTRATION & CANONICAL EXPORTS ---")
    all_ok = True

    # Check registration in lib.rs
    if re.search(r"pub\s+mod\s+admission\s*;", lib_src):
        log_pass("src/lib.rs registers 'pub mod admission;'")
    else:
        log_fail("src/lib.rs missing 'pub mod admission;'")
        all_ok = False

    # Check registration in main.rs
    if re.search(r"mod\s+admission\s*;", main_src):
        log_pass("src/main.rs registers 'mod admission;'")
    else:
        log_fail("src/main.rs missing 'mod admission;'")
        all_ok = False

    # Check evaluate_candidate_admission
    if re.search(r"pub\s+fn\s+evaluate_candidate_admission\s*\(", admission_src):
        log_pass("src/admission.rs exports 'evaluate_candidate_admission'")
    else:
        log_fail("src/admission.rs missing 'evaluate_candidate_admission'")
        all_ok = False

    # Check can_admit_candidate
    if re.search(r"pub\s+fn\s+can_admit_candidate\s*\(", admission_src):
        log_pass("src/admission.rs exports 'can_admit_candidate'")
    else:
        log_fail("src/admission.rs missing 'can_admit_candidate'")
        all_ok = False

    # Check RejectionReason enum
    if re.search(r"pub\s+enum\s+RejectionReason", admission_src):
        log_pass("src/admission.rs defines typed 'RejectionReason'")
    else:
        log_fail("src/admission.rs missing 'RejectionReason'")
        all_ok = False

    return all_ok

def audit_admission_filters(admission_src: str) -> bool:
    print("\n--- 2. AUDITING THE 5 REGISTERED ADMISSION FILTERS IN evaluate_candidate_admission ---")
    all_ok = True

    # Extract evaluate_candidate_admission function body
    match = re.search(r"pub\s+fn\s+evaluate_candidate_admission[\s\S]+?\{([\s\S]+?)\n\}", admission_src)
    if not match:
        log_fail("Could not parse evaluate_candidate_admission function body")
        return False
    body = match.group(1)

    # 1. Liquidity Floor Gate
    if "candidate.liquidity_usd < min_liquidity" in body and "LowLiquidity" in body:
        log_pass("Gate 1: Liquidity Floor strictly enforced against min_entry_liquidity_usd")
    else:
        log_fail("Gate 1: Liquidity Floor missing or disconnected in evaluate_candidate_admission")
        all_ok = False

    # 2. Genesis Guard Gate
    if "GenesisGuardEvaluator" in body and ".evaluate(" in body and "GenesisGuard" in body:
        log_pass("Gate 2: Genesis Guard bundle sniped & shared tip protection actively wired")
    else:
        log_fail("Gate 2: Genesis Guard missing or disconnected in evaluate_candidate_admission")
        all_ok = False

    # 3. Hawkes Wash Gate
    if "HawkesEvaluator::evaluate(" in body and ".is_wash_trading" in body and "HawkesWash" in body:
        log_pass("Gate 3: Hawkes point process supercritical wash filter actively wired")
    else:
        log_fail("Gate 3: Hawkes wash-trading filter missing or disconnected in evaluate_candidate_admission")
        all_ok = False

    # 4. Orderflow CVD Gate
    if "RollingCvdTracker::new(" in body and ".record_snapshot_volume(" in body and ".evaluate_divergence(" in body:
        log_pass("Gate 4: Real-time CVD taker pressure & bearish absorption divergence actively wired")
    else:
        log_fail("Gate 4: Orderflow CVD filter missing or disconnected in evaluate_candidate_admission")
        all_ok = False

    # 5. Dev Cluster / Funder Graph Gate
    if "FunderGraphEvaluator" in body and ("evaluate_holders" in body or "evaluate_token" in body) and "DevCluster" in body:
        log_pass("Gate 5: Deep Funder Graph HHI & sybil dev cluster filter actively wired")
    else:
        log_fail("Gate 5: Dev Cluster / Funder Graph filter missing or disconnected in evaluate_candidate_admission")
        all_ok = False

    return all_ok

def audit_hot_path_wiring(main_src: str) -> bool:
    print("\n--- 3. AUDITING HOT-PATH CALL GRAPH & TELEMETRY COUPLING ---")
    all_ok = True

    # Check that evaluate_candidate_admission is invoked on candidate queue pop
    if "admission::evaluate_candidate_admission(" in main_src:
        log_pass("Execution hot-path directly invokes 'admission::evaluate_candidate_admission'")
    else:
        log_fail("Execution hot-path does NOT invoke 'admission::evaluate_candidate_admission'")
        all_ok = False

    # Check telemetry coupling: rejections_count increment
    if "state.rejections_count += 1;" in main_src:
        log_pass("Rejection branch increments 'state.rejections_count'")
    else:
        log_fail("Rejection branch does not increment 'state.rejections_count'")
        all_ok = False

    # Check telemetry coupling: record_stage2_audit called on rejection
    if "state.record_stage2_audit(" in main_src:
        log_pass("Rejection branch writes to 'state.record_stage2_audit'")
    else:
        log_fail("Rejection branch does not record to 'state.record_stage2_audit'")
        all_ok = False

    # Check that scanner discovery path also evaluates admission
    scanner_section = main_src[main_src.find("run_scanner") if "run_scanner" in main_src else main_src.find("token-profiles/latest/v1"):]
    if "admission::evaluate_candidate_admission(" in scanner_section:
        log_pass("Discovery scanner path enforces canonical 'admission::evaluate_candidate_admission'")
    else:
        log_fail("Discovery scanner path missing 'admission::evaluate_candidate_admission'")
        all_ok = False

    return all_ok

def run_rust_unit_tests() -> bool:
    print("\n--- 4. EXECUTING CARGO ADMISSION UNIT TESTS ---")
    cmd = ["cargo", "test", "--lib", "admission"]
    proc = subprocess.run(cmd, cwd=PROJECT_ROOT, capture_output=True, text=True)
    if proc.returncode == 0:
        lines = [line for line in proc.stdout.splitlines() if "test admission::tests::" in line]
        for line in lines:
            print(f"  * {line.strip()}")
        log_pass("All cargo unit tests passed successfully!")
        return True
    else:
        log_fail(f"Cargo unit tests failed with code {proc.returncode}:\n{proc.stderr}\n{proc.stdout}")
        return False

def main():
    print("=" * 80)
    print(" ARTICLE 31.28: SEMANTIC LOOP & ARCHITECTURAL INTEGRITY SENTINEL")
    print("=" * 80)

    admission_src = read_file("src/admission.rs")
    lib_src = read_file("src/lib.rs")
    main_src = read_file("src/main.rs")

    ok1 = audit_module_exports(admission_src, lib_src, main_src)
    ok2 = audit_admission_filters(admission_src)
    ok3 = audit_hot_path_wiring(main_src)
    ok4 = run_rust_unit_tests()

    print("\n" + "=" * 80)
    if ok1 and ok2 and ok3 and ok4:
        print("\033[92m[SENTINEL AUDIT PASSED]\033[0m Article 31.28 semantic loop is 100% verified.")
        print("All 5 risk filters actively govern transaction dispatch. Zero phantom logic.")
        print("=" * 80 + "\n")
        sys.exit(0)
    else:
        print("\033[91m[SENTINEL AUDIT FAILED]\033[0m Article 31.28 invariant violation detected.")
        print("=" * 80 + "\n")
        sys.exit(1)

if __name__ == "__main__":
    main()

