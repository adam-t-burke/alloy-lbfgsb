//! Resumable control flow shared by all public solve entry points (ADR-001).
//!
//! A run alternates finite value/gradient submissions and accepted-iterate decisions.
//! Search arithmetic and history updates are unchanged from the callback driver.
//! Workspace buffers are borrowed; suspending a run neither spawns work nor allocates.
//! The caller owns external caches and must associate them with the requested point.

use crate::bounds::projected_gradient_and_active_count_unchecked;
use crate::core::{feasible_maximum_step, DcSearch, SearchConfig, SearchResult};
use crate::session::{DirectionError, Session};
#[cfg(feature = "benchmark-instrumentation")]
use crate::solver::elapsed_nanoseconds;
use crate::{
    Bounds, Control, Convergence, EvaluationError, Failure, Iteration, Report, SolveError, Solver,
    Stats, StopReason, Termination,
};
use std::convert::Infallible;
use std::fmt;
#[cfg(feature = "benchmark-instrumentation")]
use std::time::Instant;

/// Why an objective/gradient evaluation is requested.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EvaluationKind {
    /// The projected initial point.
    Initial,
    /// A line-search trial that may be rejected.
    Trial,
}

/// Scheduling information for one requested point; does not alter the objective.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct EvaluationContext {
    /// Initial point or line-search trial.
    pub kind: EvaluationKind,
    /// One-based number of this evaluation request.
    pub evaluation: usize,
    /// Number of major iterations already accepted.
    pub accepted_iterations: usize,
    /// Trial multiplier of the search direction; zero at the initial point.
    pub step_length: f64,
}

/// Borrowed point and writable gradient for a pending evaluation.
///
/// Overwrite every gradient entry, then call [`Run::submit`] with the objective.
/// Repeated inspection returns the same request and does not consume an evaluation.
/// Keep the objective and its parameters fixed for the entire run.
#[derive(Debug)]
#[non_exhaustive]
pub struct Evaluation<'a> {
    /// Coordinates to evaluate; never modify them independently of the run.
    pub x: &'a [f64],
    /// Solver-owned output buffer, filled with NaNs when the request is issued.
    pub gradient: &'a mut [f64],
    /// Request metadata.
    pub context: EvaluationContext,
}

/// Current run state. Inspecting it performs no numerical work.
#[derive(Debug)]
#[non_exhaustive]
pub enum RunState<'a> {
    /// Compute a value and gradient, then submit them.
    Evaluate(Evaluation<'a>),
    /// Observe this accepted point and call [`Run::advance`].
    Accepted(Iteration<'a>),
    /// Terminal report; subsequent inspection returns the same report.
    Finished(Report),
}

/// Invalid state transition or invalid submitted evaluation.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum RunError {
    /// This operation requires a pending evaluation.
    ExpectedEvaluation,
    /// This operation requires an accepted-iterate decision.
    ExpectedAccepted,
    /// Submitted value or gradient was not finite. The run remains pending so
    /// the caller can repair it or abort; no history or counter was updated.
    Evaluation(EvaluationError),
}
impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExpectedEvaluation => f.write_str("run is not awaiting an evaluation"),
            Self::ExpectedAccepted => {
                f.write_str("run is not awaiting an accepted-iterate decision")
            }
            Self::Evaluation(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for RunError {}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Initial,
    Trial,
    Accepted,
    Finished,
}

/// An exclusively borrowed, resumable L-BFGS-B solve.
///
/// Created by [`Solver::start`]. Each `submit` or `advance` performs numerical
/// work only until the next public state. No worker thread is used. On drop or
/// [`Self::abort`], pending trial coordinates are restored to the last accepted
/// point. An initial request has no evaluated accepted point to restore.
/// Workspace final-gradient results are committed only on a finished report;
/// dropping an unfinished run preserves the previous successful result.
/// Under benchmark instrumentation, time spent waiting for a submission or an
/// accepted decision is counted as evaluation or observer time, respectively.
pub struct Run<'a> {
    solver: &'a mut Solver,
    x: &'a mut [f64],
    session: Session,
    phase: Phase,
    search: Option<DcSearch>,
    value: f64,
    old_value: f64,
    gdold: f64,
    pg: f64,
    active: usize,
    iterations: usize,
    evaluations: usize,
    probes: usize,
    search_probes: usize,
    step: f64,
    report: Option<Report>,
    #[cfg(feature = "benchmark-instrumentation")]
    started: Instant,
    #[cfg(feature = "benchmark-instrumentation")]
    waiting: Instant,
}

impl<'a> Run<'a> {
    pub(crate) fn new(
        solver: &'a mut Solver,
        x: &'a mut [f64],
        bounds: Bounds<'_>,
    ) -> Result<Self, SolveError<Infallible>> {
        bounds.validate(x.len())?;
        if let Some(index) = x.iter().position(|value| !value.is_finite()) {
            return Err(SolveError::InvalidInitialValue { index });
        }
        for ((xi, &lower), &upper) in x.iter_mut().zip(bounds.lower()).zip(bounds.upper()) {
            *xi = xi.max(lower).min(upper);
        }
        solver.workspace.prepare(
            bounds.lower(),
            bounds.upper(),
            solver.options.history_size(),
        )?;
        solver.workspace.evaluation_gradient.fill(f64::NAN);
        #[cfg(feature = "benchmark-instrumentation")]
        {
            solver.instrumentation = crate::BenchmarkTimings::default();
        }
        let session = Session::new(x.len(), solver.options.history_size(), solver.kernel);
        Ok(Self {
            solver,
            x,
            session,
            phase: Phase::Initial,
            search: None,
            value: f64::NAN,
            old_value: f64::NAN,
            gdold: 0.0,
            pg: 0.0,
            active: 0,
            iterations: 0,
            evaluations: 0,
            probes: 0,
            search_probes: 0,
            step: 0.0,
            report: None,
            #[cfg(feature = "benchmark-instrumentation")]
            started: Instant::now(),
            #[cfg(feature = "benchmark-instrumentation")]
            waiting: Instant::now(),
        })
    }

    /// Borrows the current request, accepted point, or terminal report.
    #[inline]
    pub fn state(&mut self) -> RunState<'_> {
        match self.phase {
            Phase::Initial | Phase::Trial => {
                let context = EvaluationContext {
                    kind: if self.phase == Phase::Initial {
                        EvaluationKind::Initial
                    } else {
                        EvaluationKind::Trial
                    },
                    evaluation: self.evaluations + 1,
                    accepted_iterations: self.iterations,
                    step_length: self.step,
                };
                let gradient = if self.phase == Phase::Initial {
                    &mut self.solver.workspace.evaluation_gradient
                } else {
                    &mut self.solver.workspace.trial_gradient
                };
                RunState::Evaluate(Evaluation {
                    x: self.x,
                    gradient,
                    context,
                })
            }
            Phase::Accepted => RunState::Accepted(Iteration {
                x: self.x,
                gradient: &self.solver.workspace.evaluation_gradient,
                value: self.value,
                projected_gradient_norm: self.pg,
                step_length: self.step,
                previous_x: &self.solver.workspace.old_x,
                stats: self.stats(),
            }),
            Phase::Finished => RunState::Finished(self.report.expect("finished report invariant")),
        }
    }

    /// Submits the value for the pending point after its gradient was written.
    /// Invalid submissions leave the request pending without advancing the solver.
    #[inline]
    pub fn submit(&mut self, value: f64) -> Result<(), RunError> {
        if !matches!(self.phase, Phase::Initial | Phase::Trial) {
            return Err(RunError::ExpectedEvaluation);
        }
        if !value.is_finite() {
            return Err(RunError::Evaluation(EvaluationError::NonFiniteValue));
        }
        let gradient = if self.phase == Phase::Initial {
            &self.solver.workspace.evaluation_gradient
        } else {
            &self.solver.workspace.trial_gradient
        };
        if let Some(index) = gradient.iter().position(|g| !g.is_finite()) {
            return Err(RunError::Evaluation(EvaluationError::NonFiniteGradient {
                index,
            }));
        }
        #[cfg(feature = "benchmark-instrumentation")]
        {
            self.solver.instrumentation.evaluation_nanoseconds += elapsed_nanoseconds(self.waiting);
        }
        self.evaluations += 1;
        if self.phase == Phase::Initial {
            self.value = value;
            self.refresh_gradient_norm();
            if self.gradient_converged() {
                self.finish(Termination::Converged(Convergence::ProjectedGradient));
            } else {
                self.begin_direction();
            }
        } else {
            self.probes += 1;
            self.search_probes += 1;
            let gd = dot(
                &self.solver.workspace.trial_gradient,
                &self.solver.workspace.direction,
            );
            match self
                .search
                .as_mut()
                .expect("trial search invariant")
                .advance(value, gd)
            {
                SearchResult::Evaluate(_) => self.request_trial(),
                SearchResult::Converged(_) | SearchResult::Warning { .. } => {
                    self.value = value;
                    self.solver
                        .workspace
                        .evaluation_gradient
                        .copy_from_slice(&self.solver.workspace.trial_gradient);
                    self.iterations += 1;
                    self.refresh_gradient_norm();
                    self.phase = Phase::Accepted;
                    #[cfg(feature = "benchmark-instrumentation")]
                    {
                        self.waiting = Instant::now();
                    }
                }
            }
        }
        Ok(())
    }

    /// Continues from an accepted point or stops with a user-request report.
    /// Built-in convergence tests run after the accepted-point decision, as in
    /// the callback API. An initial stationary point finishes without this event.
    #[inline]
    pub fn advance(&mut self, control: Control) -> Result<(), RunError> {
        if self.phase != Phase::Accepted {
            return Err(RunError::ExpectedAccepted);
        }
        #[cfg(feature = "benchmark-instrumentation")]
        {
            self.solver.instrumentation.observer_nanoseconds += elapsed_nanoseconds(self.waiting);
        }
        let termination = if control == Control::Stop {
            Some(Termination::Stopped(StopReason::User))
        } else if self.gradient_converged() {
            Some(Termination::Converged(Convergence::ProjectedGradient))
        } else if self.solver.options.relative_function_tolerance() > 0.0
            && self.old_value - self.value
                <= self.solver.options.relative_function_tolerance()
                    * self.old_value.abs().max(self.value.abs()).max(1.0)
        {
            Some(Termination::Converged(Convergence::RelativeFunction))
        } else if self.evaluations >= self.solver.options.max_evaluations() {
            Some(Termination::Stopped(StopReason::MaximumEvaluations))
        } else {
            None
        };
        if let Some(reason) = termination {
            self.finish(reason);
        } else {
            #[cfg(feature = "benchmark-instrumentation")]
            let started = Instant::now();
            self.session
                .update(self.x, self.gdold, &mut self.solver.workspace);
            #[cfg(feature = "benchmark-instrumentation")]
            {
                self.solver.instrumentation.history_update_nanoseconds +=
                    elapsed_nanoseconds(started);
            }
            self.begin_direction();
        }
        Ok(())
    }

    /// Consumes an unfinished run and restores any pending trial coordinates.
    /// Does not fabricate convergence or overwrite the previous final gradient.
    pub fn abort(self) {}

    fn gradient_converged(&self) -> bool {
        self.solver.options.projected_gradient_tolerance() > 0.0
            && self.pg <= self.solver.options.projected_gradient_tolerance()
    }
    fn refresh_gradient_norm(&mut self) {
        (self.pg, self.active) = projected_gradient_and_active_count_unchecked(
            self.x,
            &self.solver.workspace.evaluation_gradient,
            &self.solver.workspace.lower,
            &self.solver.workspace.upper,
        );
    }
    fn stats(&self) -> Stats {
        Stats {
            iterations: self.iterations,
            evaluations: self.evaluations,
            line_search_probes: self.probes,
            accepted_updates: self.session.accepted_updates(),
            skipped_updates: self.session.skipped_updates(),
            history_resets: self.session.history_resets(),
            active_variables: self.active,
            free_variables: self.x.len() - self.active,
        }
    }
    fn finish(&mut self, termination: Termination) {
        self.report = Some(Report {
            value: self.value,
            projected_gradient_norm: self.pg,
            stats: self.stats(),
            termination,
        });
        // prepare reserves this result buffer, so committing cannot allocate.
        self.solver
            .workspace
            .commit_gradient()
            .expect("prepared final-gradient capacity");
        self.phase = Phase::Finished;
        #[cfg(feature = "benchmark-instrumentation")]
        self.solver.finish_instrumentation(self.started);
    }
    fn rollback(&mut self) {
        self.x.copy_from_slice(&self.solver.workspace.old_x);
        self.value = self.old_value;
        self.solver
            .workspace
            .evaluation_gradient
            .copy_from_slice(&self.solver.workspace.old_gradient);
    }
    fn begin_direction(&mut self) {
        loop {
            if self.iterations >= self.solver.options.max_iterations() {
                self.finish(Termination::Stopped(StopReason::MaximumIterations));
                return;
            }
            if self.evaluations >= self.solver.options.max_evaluations() {
                self.finish(Termination::Stopped(StopReason::MaximumEvaluations));
                return;
            }
            #[cfg(feature = "benchmark-instrumentation")]
            let started = Instant::now();
            let direction = self.session.direction(self.x, &mut self.solver.workspace);
            #[cfg(feature = "benchmark-instrumentation")]
            {
                self.solver.instrumentation.direction_nanoseconds += elapsed_nanoseconds(started);
                let t = self.session.benchmark_timings();
                self.solver.instrumentation.cauchy_nanoseconds = t.cauchy_nanoseconds;
                self.solver.instrumentation.freev_nanoseconds = t.freev_nanoseconds;
                self.solver.instrumentation.formk_nanoseconds = t.formk_nanoseconds;
                self.solver.instrumentation.cmprlb_nanoseconds = t.cmprlb_nanoseconds;
                self.solver.instrumentation.subsm_nanoseconds = t.subsm_nanoseconds;
            }
            if let Err(error) = direction {
                self.finish(Termination::Failed(match error {
                    DirectionError::NoDescent => Failure::NoDescentDirection,
                    DirectionError::Factorization => Failure::Factorization,
                    DirectionError::Numerical => Failure::Numerical,
                }));
                return;
            }
            self.solver.workspace.old_x.copy_from_slice(self.x);
            self.solver
                .workspace
                .old_gradient
                .copy_from_slice(&self.solver.workspace.evaluation_gradient);
            self.old_value = self.value;
            self.gdold = dot(
                &self.solver.workspace.old_gradient,
                &self.solver.workspace.direction,
            );
            let maximum = feasible_maximum_step(
                &self.solver.workspace.old_x,
                &self.solver.workspace.direction,
                &self.solver.workspace.lower,
                &self.solver.workspace.upper,
                self.iterations,
            );
            let norm = dot(
                &self.solver.workspace.direction,
                &self.solver.workspace.direction,
            )
            .sqrt();
            let initial = if self.iterations == 0 {
                let boxed = self
                    .solver
                    .workspace
                    .lower
                    .iter()
                    .zip(&self.solver.workspace.upper)
                    .all(|(&l, &u)| l.is_finite() && u.is_finite());
                if boxed {
                    1.0
                } else {
                    (1.0 / norm).min(maximum)
                }
            } else {
                1.0
            };
            match DcSearch::start(
                self.value,
                self.gdold,
                initial,
                SearchConfig::lbfgsb(maximum),
            ) {
                Ok(search) => {
                    self.search = Some(search);
                    self.search_probes = 0;
                    self.request_trial();
                    return;
                }
                Err(_) if self.session.has_history() => self.session.reset_history(),
                Err(_) => {
                    self.finish(Termination::Failed(Failure::LineSearch));
                    return;
                }
            }
        }
    }
    fn request_trial(&mut self) {
        if self.search_probes >= self.solver.options.max_line_search_evaluations() {
            self.rollback();
            if self.session.has_history() {
                self.session.reset_history();
                self.begin_direction();
            } else {
                self.finish(Termination::Stopped(
                    StopReason::MaximumLineSearchEvaluations,
                ));
            }
            return;
        }
        if self.evaluations >= self.solver.options.max_evaluations() {
            self.rollback();
            self.finish(Termination::Stopped(StopReason::MaximumEvaluations));
            return;
        }
        self.step = self
            .search
            .as_ref()
            .expect("search request invariant")
            .step();
        if self.step == 1.0 {
            self.x.copy_from_slice(&self.solver.workspace.cauchy);
        } else {
            for i in 0..self.x.len() {
                self.x[i] = (self.solver.workspace.old_x[i]
                    + self.step * self.solver.workspace.direction[i])
                    .max(self.solver.workspace.lower[i])
                    .min(self.solver.workspace.upper[i]);
            }
        }
        self.solver.workspace.trial_gradient.fill(f64::NAN);
        self.phase = Phase::Trial;
        #[cfg(feature = "benchmark-instrumentation")]
        {
            self.waiting = Instant::now();
        }
    }
}
impl Drop for Run<'_> {
    fn drop(&mut self) {
        if self.phase == Phase::Trial {
            self.rollback();
        }
    }
}
fn dot(left: &[f64], right: &[f64]) -> f64 {
    left.iter().zip(right).map(|(&a, &b)| a * b).sum()
}
