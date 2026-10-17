# ADR-002: Isolate driver costs before tuning

Status: Accepted
Date: 2026-10-17

## Context
ADR-001 measured a 3.18% whole-solve slowdown on one unpinned case. It did not
isolate state-machine cost from added gradient sentinel writes or code generation.

## Investigation
Use isolated temporary worktrees: pre-refactor 9a200cb, current ca6baa5, current
with only NaN buffer fills removed. Compile the same benchmark harness for all.
Compare both backends, several dimensions and objective/bound patterns with
rotating execution order, fixed affinity where available, warm reusable solvers,
multiple process samples, and recorded numerical fingerprints/counters.
Run separate instrumented trials to attribute coarse phases; do not use their
timings as release overhead measurements. Preserve raw data and tools.

## Decision criteria
Do not assume all observed differences come from the public interface. Keep
valid-input arithmetic/reduction order, line search, fixtures, and termination
unchanged. Retain one shared engine and zero warm allocations. Test concrete
optimizations in isolated variants before adopting them; reject changes that
alter deterministic trajectories. A validation-policy change needs an explicit
record of its user-visible contract. No speculative parallelism or reassociation.

## Validation
Reference, protocol, allocation, scalar/all-feature and MSRV tests; strict lint,
formatting and package verification. Report repeatability and test limitations.
