# Status

2026-10-25: Version 0.1.0 is the first crates.io release of this tree. No earlier
version, tag, or GitHub release existed. The published API is the callback solver
and the resumable `Solver::start` / `RunState` API on the deterministic scalar
backend. Optional `faer-backend` and `benchmark-instrumentation` stay opt-in.
Normal `cargo build` and `cargo test` use the committed CSV fixtures. The Fortran
regenerators in `tests/reference` are packaged with those fixtures and are not
invoked by the build or the tests. `tools/README.md` is packaged because these
notes link to it. The profiling scripts and variant patches under `tools/` are
not packaged, and neither they nor a Fortran compiler are required to build or
test the crate.

2026-10-17: ADR-001 resumable interface and ADR-002 profiling/tuning implemented.

- One numerical engine serves callback and resumable borrowing APIs.
- Derivative reuse and conditional norm computation remove redundant reductions.
- Scalar history products share a traversal with independent ordered accumulators.
- Gradient sentinel validation, line search, history and stopping policies are unchanged.
- Stable debug/release, Rust 1.80 scalar/all-feature, independent Fortran/reference,
  protocol, bitwise-product and zero-warm-allocation tests pass locally.
- Strict formatting/Clippy/rustdoc and package verification pass. Remote CI has not run.

## Performance evidence and open issue

The initial 3.18% result was whole-benchmark slowdown, not isolated driver overhead.
Controlled final comparisons show deterministic Rosenbrock gains of 5.42% at n=1000
and 5.56% at n=8192 versus the pre-refactor original. This is not universal: the
100,000-variable faer mixed-bound case remains 2.53% slower than the original.
An old driver with the same optimizations/validation remains faster in equal-policy
controls; large-case gaps are around 4-7% across repeated experiments. Exact machine-code
attribution and parity remain open. Neither sentinel removal, direct-wrapper bypass,
nor forced transition inlining consistently resolved it; those changes were rejected.

Read [ADR-002 validation](validation/ADR-002.md) and [benchmarks](../BENCHMARKS.md).
Reproduction scripts are described in `tools/README.md`. Cargo packages that
linked note, not the scripts or variant patches beside it. All sample ranges
and rejected controls are retained.
ADR-001's historical checks remain in [its report](validation/ADR-001.md).

Next: inspect equal-policy generated-code/alias/bounds-check/vectorization differences
in driver and free-set/subspace hot loops. Scaling adapters, alternate line searches,
history recycling, serialized checkpoints and application AL policy remain separate work.
