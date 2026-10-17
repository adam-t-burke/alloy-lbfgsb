# Status

2026-10-17: standalone extraction cleanup and ADR-001 implemented.

- One numerical engine serves callback and resumable borrowing APIs.
- Evaluation context, accepted gradients, lazy step norms and workspace capacity reporting added.
- Callback/resumable zero-allocation and independent oracle tests pass locally.
- Stable/MSRV checks, package verification and measured performance evidence are recorded in
  [ADR-001 validation](validation/ADR-001.md) and [BENCHMARKS.md](../BENCHMARKS.md).
- Standalone CI, lockfile, attribution inventory and LF policy restored; remote CI pending.
- About 3% driver overhead on the measured n=1000,m=10 case; not a speed improvement.

Next candidates need their own ADR: explicit scaling adapters; richer evaluation-failure
policies; representative large-n profiling and backend tuning. No objective-only line search,
history recycling, serialized checkpoints, application AL policy or voxtop integration added.
Historical benchmarks remain inherited measurements, distinguished from new raw results.
