# ADR-001: Resumable solver with a shared numerical engine

Status: Accepted
Date: 2026-10-17

## Context
Expensive simulation callers need explicit evaluation scheduling and accepted-point
observations. The existing synchronous callback API hides scheduling inside minimize.
The extracted solver already has independent Fortran fixtures and warm allocation tests.

## Decision
Expose a borrowing Run with explicit Evaluate, Accepted, and Finished states. The caller
fills a borrowed gradient buffer and submits a finite value, or aborts. Evaluate exposes
initial/trial context and the step length. Accepted exposes the accepted gradient, step
length and infinity norm of displacement. A single state machine drives both this API
and existing callbacks; preserve arithmetic, line search, limits, and history policy.
No threads, async runtime, dynamic dispatch, or per-step allocations are introduced.
Each run resets curvature history while retaining workspace capacity. Dropping a run
restores the last accepted coordinates during a pending trial. Callback/evaluation errors
abort and restore accepted coordinates; external evaluation caches remain caller-owned.
No trial evaluation is automatically retried after an application error.

## Alternatives and scope
A second reverse-communication engine risks divergent numerical behavior. Objective-only
probes require a different line-search policy and are deferred. Scaling adapters, persistent
checkpoints, history recycling, and parallel reductions need separate decisions/evidence.
AL policies and constrained optimality belong to callers. Built-in function-reduction and
projected-gradient reasons remain distinct; either tolerance can already be disabled.

## Compatibility
Retain callback entry points and their defaults. New Iteration fields are additive but
break external struct literals in this pre-1.0 crate; document them and make future report
views non-exhaustive. Keep pure Rust scalar and optional faer backends.

## Validation
Run scalar/all-feature/reference/allocation/doc tests, formatting, strict clippy,
package dry run, callback-versus-resumable equivalence, invalid-evaluation recovery,
cancellation and budget tests. Compare representative release timings against the parent.
Record measurements and remaining limits in docs/STATUS.md.
