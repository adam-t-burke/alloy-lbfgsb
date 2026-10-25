# Changelog

## 0.1.0 - 2026-10-25

First crates.io release. The crates.io index, the git tags, and the GitHub
releases for this repository were empty, so manifest version 0.1.0 had not
identified a published API. This release publishes the current source: the
callback `Solver::minimize` API and the resumable `Solver::start` / `RunState`
API, both served by one numerical engine. The default build has no optional
features. `Backend::Auto` resolves to `Backend::Deterministic`.

`Iteration` is non-exhaustive in this first published API. Obtain it from the
solver callbacks or accepted-state views rather than a struct literal.

- Add a resumable borrowing Run API sharing the callback numerical engine.
- Add evaluation context and borrowed accepted gradients, step lengths and norms.
- Reject incomplete gradient writes; restore accepted coordinates on trial cancellation.
- Restore standalone CI, release benchmark settings and ADR-based development guidance.
- Reuse validated directional derivatives and avoid unused direction-norm reductions.
- Pair independent scalar history products with unchanged accumulation order.
- Add controlled driver profiling, exact experiment patches and explicit remaining-overhead evidence.
