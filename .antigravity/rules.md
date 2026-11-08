# Antigravity Trading System Rules & Invariants

## Article 31.28: Full-Suite Semantic Loop & Architectural Integrity Sentinel

1. **Zero Disconnected Modules (No Phantom Logic)**:
   - Every risk gate, microstructure filter, or security check (Hawkes Wash, Dev Cluster, Orderflow CVD, Genesis Guard, Liquidity Floor) defined in the codebase MUST be strictly wired into the live candidate evaluation hot-path (`can_admit_candidate` / `evaluate_candidate_admission`).
   - Logging, shadow telemetry, or cold-path simulation alone DOES NOT constitute an active filter. If a metric can reject a trade counterfactually, it MUST be capable of returning `Ok(false)` or `Err(RejectionReason)` on the live orderflow path.
   - Zero defensive modules may exist without an active call in the hot-path admission loop.

2. **Hot-Path Call Graph Invariant**:
   - Any commit modifying filters or admission rules must pass `scripts/audit_semantic_loop_sentinel.py`.
   - The sentinel verifies that the admission predicate directly calls the verification functions of all registered filters and that their return values actively govern transaction dispatch.

3. **Rejection Counter Telemetry Coupling**:
   - Rolling rejection metrics displayed on the dashboard MUST increment directly from live candidate rejections in the hot path, never from mocked or decoupled counters.
   - Any token with Hawkes branching ratio >= 1.15 or dev cluster risk MUST be immediately rejected with `Stage2Rejection::HawkesWash` or `Stage2Rejection::DevCluster`.

