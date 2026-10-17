# Repository guidance

- Read README.md, docs/STATUS.md and the applicable docs/adr decision first.
- Keep this a general-purpose pure Rust bound-constrained L-BFGS-B library.
  Application physics, AL updates, and domain-specific convergence belong to callers.
- Record API, numerical-policy, or backend decisions in numbered ADRs before coding.
  Update their implementation status and validation evidence when complete.
- One numerical implementation serves synchronous and resumable APIs. Preserve the
  default algorithm and arithmetic unless a separate decision justifies a change.
- Keep f64 slice APIs, typed failures, and documented bounds, scaling and stopping semantics.
  Public APIs must document invalid transitions and ownership of accepted/trial state.
- No hidden threads, global mutable solver state, FFI, or per-step allocations after warmup.
  Justify unsafe code locally; prefer safe Rust. Keep optional acceleration explicit.
- Never claim convergence from budget exhaustion or equate function stagnation with KKT.
- Validate with independent Fortran fixtures, algebraic cases, and allocation tests.
  Never regenerate oracle fixtures merely to make a changed implementation pass.
- Performance claims require release settings, hardware, dimensions/history, correctness,
  evaluation counts and repeated timings. Preserve deterministic scalar reduction order.
- Run cargo fmt --check, cargo clippy --all-targets --all-features -- -D warnings,
  cargo test --no-default-features, cargo test --all-features, and package checks.
  Verify the declared MSRV separately; use uv for Python tooling.
- Work on a descriptive codex/ branch; commit separate logical changes. Preserve unrelated
  edits. Do not force-push, amend published commits, merge, or publish without authorization.
- Keep instructions concise. Do not claim checks passed unless run and observed.
