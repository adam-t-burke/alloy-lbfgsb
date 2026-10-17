# Changelog

## Unreleased

- Add a resumable borrowing Run API sharing the callback numerical engine.
- Add evaluation context and borrowed accepted gradients, step lengths and norms.
- Reject incomplete gradient writes; restore accepted coordinates on trial cancellation.
- Make Iteration non-exhaustive (external struct literals require migration).
- Restore standalone CI, release benchmark settings and ADR-based development guidance.

- Reuse validated directional derivatives and avoid unused direction-norm reductions.
- Pair independent scalar history products with unchanged accumulation order.
- Add controlled driver profiling, exact experiment patches and explicit remaining-overhead evidence.
