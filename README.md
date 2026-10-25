# alloy-lbfgsb

A standalone, callback-first interface for bound-constrained L-BFGS-B.

The minimum supported Rust version is 1.80 for both the scalar implementation
and the optional `faer-backend` feature.

Version 0.1.0 is the first crates.io release. No earlier version was published.
Depend on the deterministic scalar backend by leaving the optional features off:

```toml
alloy-lbfgsb = { version = "0.1.0", default-features = false }
```

`Backend::Auto` resolves to `Backend::Deterministic`. Enable `faer-backend` or
`benchmark-instrumentation` only when that optional behavior is wanted.

The [API documentation](https://docs.rs/alloy-lbfgsb) describes the complete
public interface. A runnable version of the example below is available as
`examples/quadratic.rs`:

```console
cargo run --example quadratic
```

The core API is `f64` and slice based. `Solver` owns reusable `Workspace`
storage, updates parameters in place, and accepts an allocation-free gradient
callback:

```rust
use alloy_lbfgsb::{Bounds, Options, Solver};

let mut x = [3.0, -4.0];
let bounds = Bounds::new(&[-10.0; 2], &[10.0; 2], x.len())?;
let options = Options::new()
    .with_relative_function_tolerance(1e-12)?
    .with_projected_gradient_tolerance(1e-8)?;
let mut solver = Solver::new(options);
let report = solver.minimize(&mut x, bounds, |x, gradient| {
    gradient[0] = 2.0 * (x[0] - 1.0);
    gradient[1] = 2.0 * (x[1] + 2.0);
    Ok::<_, std::convert::Infallible>(
        (x[0] - 1.0).powi(2) + (x[1] + 2.0).powi(2),
    )
})?;
assert!(report.termination.converged());
# Ok::<(), Box<dyn std::error::Error>>(())
```

The projected-gradient tolerance stops when the infinity norm of the gradient
projected onto feasible directions is at most the configured value. The
relative-function tolerance stops after an accepted iterate when
`(f[k] - f[k+1]) / max(|f[k]|, |f[k+1]|, 1)` is at most the configured value.
Initial coordinates outside their bounds are projected before the first
objective evaluation. Iteration, evaluation, and line-search limits are
independent and are applied exactly as configured.

Termination, convergence, limits, failures, statistics, option errors, bound
errors, callback errors, and evaluation-contract errors are represented by
typed enums. Backend selection is explicit through `Options::with_backend`;
`Auto` currently resolves to the deterministic scalar backend, and selecting
`Faer` requires the `faer-backend` feature.

After a solve has grown all `Workspace` buffers for a given dimension and
history size, a repeated high-level `Solver` solve performs no solver-owned,
same-thread heap allocations in workspace preparation, line search,
compact-history rollover, or report creation. User callbacks and work performed
on other threads are outside this guarantee. The serialized thread-local
allocation test in `tests/allocations.rs` covers representative history sizes
and available backends.

`Stats` reports objective evaluations, line-search probes, accepted and skipped
curvature updates, numerical history resets, and final active/free counts in
addition to accepted major iterations. Optional coarse phase timing, including
total and unattributed residual time, is available through the
`benchmark-instrumentation` feature and is compiled out otherwise.

Behavior is checked against the authoritative L-BFGS-B 3.0 Fortran release.

This crate is licensed exclusively under BSD-3-Clause; see `LICENSE`.
The verbatim license file from the upstream L-BFGS-B 3.0 distribution is
preserved as `UPSTREAM_LICENSE.txt`; attribution and citations are in
`THIRD_PARTY_NOTICES.md`.

The committed reference trajectories and their opt-in regeneration harness
live in `tests/reference`. Normal Cargo tests consume the CSV fixtures and do
not require a Fortran compiler. See `tests/reference/PROVENANCE.md` for the
pinned upstream archive, SHA-256, compiler flags, objective definitions,
fixture hashes, and reproducibility limitations.

Release benchmark cases, commands, the official-Fortran comparison harness,
and recorded measurements are in [BENCHMARKS.md](BENCHMARKS.md).

## Resumable evaluations and monitoring

`Solver::start` returns a borrowing `Run`. Inspect `Run::state` to receive an
`Evaluate` request, an `Accepted` view, or a `Finished` report. Fill the supplied
gradient and submit the objective with `Run::submit`; continue an accepted point
with `Run::advance(Control::Continue)`. See `cargo run --example resumable`.
The callback APIs drive this same engine and remain the simplest synchronous API.

Requests include initial/trial context, a one-based evaluation number, accepted
iteration count and trial step length. `SolveAdapter::value_and_gradient_with_context`
provides the same metadata to synchronous callers; its default calls the original
callback. Accepted views borrow the gradient and report step length and the actual
infinity norm of displacement via the O(n), on-demand `Iteration::step_norm()`. Repeated state inspection does not advance the run.
No executor, worker threads or per-step heap allocation is introduced.

Every requested gradient buffer is filled with NaNs: callbacks must overwrite all
entries. Invalid submissions remain pending and can be repaired or aborted. Dropping
or aborting a pending trial restores accepted coordinates; external simulation caches
are caller-owned and must be invalidated or matched to their evaluated coordinates.
An initial request has no evaluated accepted point. Application callback errors still
return through `SolveError`, with trial coordinates rolled back.

Keep the objective, bounds and all objective parameters fixed within a run. Start a
new run when they change: allocation capacity is retained, curvature history is not.
For asynchronous evaluations, suspend the run in caller-owned control flow; `Run` is
an in-memory session, not a serializable checkpoint. Moving a run between threads is
subject to Rust's normal ownership and Send rules.

For stationarity-only built-in stopping, set `with_relative_function_tolerance(0.0)`
and choose a positive projected-gradient tolerance. `Convergence::RelativeFunction`
means the accepted objective reduction is small; it does not establish constrained
KKT conditions or a global minimum. The caller owns any outer constraint checks.
`Iteration` is now non-exhaustive; consumers must obtain it through callbacks rather
than constructing literals. This is an API addition with a struct-literal compatibility
change in the pre-1.0 release.

## Development

See [AGENTS.md](AGENTS.md), [status](docs/STATUS.md), and the
[ADR index](docs/adr/README.md). CI tests stable Rust and Rust 1.80 on Windows and
Linux, scalar and all-feature builds, reference fixtures and warm allocations.
`Cargo.lock` fixes repository validation dependencies; downstream libraries resolve
according to the published manifest. The crate remains pure Rust; Fortran is used
only by the opt-in reference-fixture tooling.

`Workspace::allocated_bytes()` reports owned vector capacity, excluding allocator overhead
and application memory, for capacity planning and benchmark memory reporting.
