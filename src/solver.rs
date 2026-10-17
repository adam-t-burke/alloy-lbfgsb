//! Callback-first solver API backed by the in-repo scalar engine.
#![allow(clippy::needless_range_loop)] // Preserve line-search arithmetic order.

use crate::kernel::Kernel;
#[cfg(feature = "benchmark-instrumentation")]
use crate::BenchmarkTimings;
use crate::{Backend, Bounds, Control, Iteration, Options, Report, SolveError, Workspace};
#[cfg(feature = "benchmark-instrumentation")]
use std::time::Instant;

/// Stateful objective and accepted-iteration adapter.
///
/// Keeping both hooks on one object lets applications share mutable evaluation
/// state without reference counting or interior mutability.
pub trait SolveAdapter {
    /// Application error returned by either callback.
    type Error;

    /// Computes the objective value and overwrites every gradient entry.
    fn value_and_gradient(&mut self, x: &[f64], gradient: &mut [f64]) -> Result<f64, Self::Error>;

    /// Evaluates a point with initial/trial scheduling context. The default calls
    /// [`Self::value_and_gradient`]; context must not change the objective being solved.
    fn value_and_gradient_with_context(
        &mut self,
        x: &[f64],
        gradient: &mut [f64],
        _context: crate::EvaluationContext,
    ) -> Result<f64, Self::Error> {
        self.value_and_gradient(x, gradient)
    }

    /// Observes an accepted major iteration and optionally stops the solve.
    fn accepted_iteration(&mut self, _iteration: Iteration<'_>) -> Result<Control, Self::Error> {
        Ok(Control::Continue)
    }
}

struct ClosureAdapter<F, C> {
    value_and_gradient: F,
    callback: C,
}

impl<E, F, C> SolveAdapter for ClosureAdapter<F, C>
where
    F: FnMut(&[f64], &mut [f64]) -> Result<f64, E>,
    C: FnMut(Iteration<'_>) -> Result<Control, E>,
{
    type Error = E;

    fn value_and_gradient(&mut self, x: &[f64], gradient: &mut [f64]) -> Result<f64, Self::Error> {
        (self.value_and_gradient)(x, gradient)
    }

    fn accepted_iteration(&mut self, iteration: Iteration<'_>) -> Result<Control, Self::Error> {
        (self.callback)(iteration)
    }
}

/// An L-BFGS-B solver with reusable owned workspace.
#[derive(Debug)]
pub struct Solver {
    pub(crate) options: Options,
    pub(crate) workspace: Workspace,
    pub(crate) kernel: Kernel,
    #[cfg(feature = "benchmark-instrumentation")]
    pub(crate) instrumentation: BenchmarkTimings,
}

impl Default for Solver {
    fn default() -> Self {
        Self::new(Options::default())
    }
}

impl Solver {
    /// Creates a solver with empty reusable workspace.
    pub fn new(options: Options) -> Self {
        Self {
            options,
            workspace: Workspace::new(),
            kernel: Kernel::selected(options.backend()),
            #[cfg(feature = "benchmark-instrumentation")]
            instrumentation: BenchmarkTimings::default(),
        }
    }

    /// Creates a solver using an existing reusable workspace.
    pub fn with_workspace(options: Options, workspace: Workspace) -> Self {
        Self {
            options,
            workspace,
            kernel: Kernel::selected(options.backend()),
            #[cfg(feature = "benchmark-instrumentation")]
            instrumentation: BenchmarkTimings::default(),
        }
    }

    /// Returns the active solver configuration.
    pub fn options(&self) -> Options {
        self.options
    }

    /// Returns the solver's reusable workspace.
    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    /// Returns mutable access to the solver's reusable workspace.
    pub fn workspace_mut(&mut self) -> &mut Workspace {
        &mut self.workspace
    }

    /// Returns the backend resolved for this solver.
    pub fn resolved_backend(&self) -> Backend {
        self.kernel.public()
    }

    #[cfg(feature = "benchmark-instrumentation")]
    /// Returns timings from the most recent attempted solve.
    ///
    /// `total_nanoseconds` and `residual_nanoseconds` are finalized only when
    /// the solve returns `Ok`; phase counters may be partial after an error.
    pub fn benchmark_timings(&self) -> BenchmarkTimings {
        self.instrumentation
    }

    /// Minimizes an objective, updating `x` in place.
    ///
    /// The callback must overwrite every entry in `gradient` before returning.
    pub fn minimize<E, F>(
        &mut self,
        x: &mut [f64],
        bounds: Bounds<'_>,
        value_and_gradient: F,
    ) -> Result<Report, SolveError<E>>
    where
        F: FnMut(&[f64], &mut [f64]) -> Result<f64, E>,
    {
        self.minimize_with_callback(x, bounds, value_and_gradient, |_| Control::Continue)
    }

    /// Minimizes an objective and reports each accepted major iteration.
    pub fn minimize_with_callback<E, F, C>(
        &mut self,
        x: &mut [f64],
        bounds: Bounds<'_>,
        value_and_gradient: F,
        mut callback: C,
    ) -> Result<Report, SolveError<E>>
    where
        F: FnMut(&[f64], &mut [f64]) -> Result<f64, E>,
        C: FnMut(Iteration<'_>) -> Control,
    {
        self.minimize_with_fallible_callback(x, bounds, value_and_gradient, |iteration| {
            Ok(callback(iteration))
        })
    }

    /// Minimizes an objective with a fallible accepted-iteration callback.
    pub fn minimize_with_fallible_callback<E, F, C>(
        &mut self,
        x: &mut [f64],
        bounds: Bounds<'_>,
        value_and_gradient: F,
        callback: C,
    ) -> Result<Report, SolveError<E>>
    where
        F: FnMut(&[f64], &mut [f64]) -> Result<f64, E>,
        C: FnMut(Iteration<'_>) -> Result<Control, E>,
    {
        let mut adapter = ClosureAdapter {
            value_and_gradient,
            callback,
        };
        self.minimize_with_adapter(x, bounds, &mut adapter)
    }

    /// Minimizes an objective through one stateful application adapter.
    pub fn minimize_with_adapter<A>(
        &mut self,
        x: &mut [f64],
        bounds: Bounds<'_>,
        adapter: &mut A,
    ) -> Result<Report, SolveError<A::Error>>
    where
        A: SolveAdapter,
    {
        let mut run = self.start(x, bounds).map_err(convert_start_error)?;
        loop {
            match run.state() {
                crate::RunState::Evaluate(evaluation) => {
                    let value = adapter
                        .value_and_gradient_with_context(
                            evaluation.x,
                            evaluation.gradient,
                            evaluation.context,
                        )
                        .map_err(SolveError::Objective)?;
                    run.submit(value).map_err(|error| match error {
                        crate::RunError::Evaluation(error) => SolveError::Evaluation(error),
                        _ => SolveError::Internal(error.to_string()),
                    })?;
                }
                crate::RunState::Accepted(iteration) => {
                    let control = adapter
                        .accepted_iteration(iteration)
                        .map_err(SolveError::Callback)?;
                    run.advance(control)
                        .map_err(|error| SolveError::Internal(error.to_string()))?;
                }
                crate::RunState::Finished(report) => return Ok(report),
            }
        }
    }

    /// Starts a resumable solve, projecting finite initial coordinates into the box.
    ///
    /// Each call resets curvature history but reuses allocated workspace. The returned
    /// run exclusively borrows this solver and `x` until dropped. See [`crate::Run`].
    pub fn start<'a>(
        &'a mut self,
        x: &'a mut [f64],
        bounds: Bounds<'_>,
    ) -> Result<crate::Run<'a>, SolveError<std::convert::Infallible>> {
        crate::Run::new(self, x, bounds)
    }

    #[cfg(feature = "benchmark-instrumentation")]
    pub(crate) fn finish_instrumentation(&mut self, started: Instant) {
        self.instrumentation.total_nanoseconds = elapsed_nanoseconds(started);
        let attributed = self
            .instrumentation
            .evaluation_nanoseconds
            .saturating_add(self.instrumentation.direction_nanoseconds)
            .saturating_add(self.instrumentation.observer_nanoseconds)
            .saturating_add(self.instrumentation.history_update_nanoseconds);
        self.instrumentation.residual_nanoseconds = self
            .instrumentation
            .total_nanoseconds
            .saturating_sub(attributed);
    }
}

fn convert_start_error<E>(error: SolveError<std::convert::Infallible>) -> SolveError<E> {
    match error {
        SolveError::Bounds(error) => SolveError::Bounds(error),
        SolveError::Options(error) => SolveError::Options(error),
        SolveError::Workspace(error) => SolveError::Workspace(error),
        SolveError::InvalidInitialValue { index } => SolveError::InvalidInitialValue { index },
        SolveError::Evaluation(error) => SolveError::Evaluation(error),
        SolveError::Internal(error) => SolveError::Internal(error),
        SolveError::Objective(never) | SolveError::Callback(never) => match never {},
    }
}

#[cfg(feature = "benchmark-instrumentation")]
pub(crate) fn elapsed_nanoseconds(start: Instant) -> u64 {
    start.elapsed().as_nanos().min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod adapter_tests {
    use super::*;

    struct FailingObserver {
        evaluations: usize,
    }

    impl SolveAdapter for FailingObserver {
        type Error = &'static str;

        fn value_and_gradient(
            &mut self,
            x: &[f64],
            gradient: &mut [f64],
        ) -> Result<f64, Self::Error> {
            self.evaluations += 1;
            gradient[0] = 2.0 * (x[0] - 1.0);
            Ok((x[0] - 1.0).powi(2))
        }

        fn accepted_iteration(
            &mut self,
            _iteration: Iteration<'_>,
        ) -> Result<Control, Self::Error> {
            Err("observer failure")
        }
    }

    #[test]
    fn stateful_adapter_preserves_callback_error() {
        let mut x = [3.0];
        let bounds = Bounds::new(&[-10.0], &[10.0], 1).unwrap();
        let mut adapter = FailingObserver { evaluations: 0 };
        let mut solver = Solver::default();

        let error = solver
            .minimize_with_adapter(&mut x, bounds, &mut adapter)
            .unwrap_err();

        assert!(matches!(error, SolveError::Callback("observer failure")));
        assert!(adapter.evaluations >= 2);
    }
}

#[cfg(all(test, feature = "faer-backend"))]
mod backend_tests {
    use super::*;
    use std::convert::Infallible;

    fn trajectory(kernel: Kernel) -> (Vec<(f64, f64)>, Vec<f64>, Report) {
        const N: usize = 180;
        let mut x = vec![3.0; N];
        let mut lower = vec![-100.0; N];
        let upper = vec![100.0; N];
        for i in (0..N).step_by(2) {
            lower[i] = 1.0;
        }
        let options = Options::new()
            .with_history_size(10)
            .unwrap()
            .with_projected_gradient_tolerance(1.0e-5)
            .unwrap();
        let mut solver = Solver::new(options);
        solver.kernel = kernel;
        let mut values = Vec::new();
        let report = solver
            .minimize_with_callback(
                &mut x,
                Bounds::new(&lower, &upper, N).unwrap(),
                |x, gradient| {
                    let mut value = 0.25 * (x[0] - 1.0).powi(2);
                    for i in 1..N {
                        let residual = x[i] - x[i - 1].powi(2);
                        value += residual * residual;
                    }
                    value *= 4.0;
                    let mut residual = x[1] - x[0].powi(2);
                    gradient[0] = 2.0 * (x[0] - 1.0) - 16.0 * x[0] * residual;
                    for i in 1..N - 1 {
                        let previous = residual;
                        residual = x[i + 1] - x[i].powi(2);
                        gradient[i] = 8.0 * previous - 16.0 * x[i] * residual;
                    }
                    gradient[N - 1] = 8.0 * residual;
                    Ok::<_, Infallible>(value)
                },
                |iteration| {
                    values.push((iteration.value, iteration.projected_gradient_norm));
                    Control::Continue
                },
            )
            .unwrap();
        (values, x, report)
    }

    #[test]
    fn scalar_and_faer_match_full_driver_trajectory() {
        let (scalar, scalar_x, scalar_report) = trajectory(Kernel::Scalar);
        let (faer, faer_x, faer_report) = trajectory(Kernel::Faer);
        assert_eq!(faer.len(), scalar.len());
        assert_eq!(faer_report.stats, scalar_report.stats);
        assert_eq!(faer_report.termination, scalar_report.termination);
        for ((faer_value, faer_pg), (scalar_value, scalar_pg)) in faer.iter().zip(&scalar) {
            assert!((faer_value - scalar_value).abs() <= 1.0e-12 * scalar_value.abs().max(1.0));
            assert!((faer_pg - scalar_pg).abs() <= 1.0e-11 * scalar_pg.abs().max(1.0));
        }
        for (faer, scalar) in faer_x.iter().zip(&scalar_x) {
            assert!((faer - scalar).abs() <= 1.0e-12 * scalar.abs().max(1.0));
        }
    }
}
