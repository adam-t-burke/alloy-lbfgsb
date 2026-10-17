use alloy_lbfgsb::{
    Bounds, Control, EvaluationError, EvaluationKind, Options, RunError, RunState, SolveAdapter,
    Solver, StopReason, Termination,
};
use std::convert::Infallible;

fn objective(x: &[f64], g: &mut [f64]) -> f64 {
    let mut f = 0.0;
    for i in 0..x.len() {
        let target = (i % 7) as f64 * 0.3 - 1.0;
        let residual = x[i] - target;
        let scale = 1.0 + (i % 13) as f64;
        g[i] = scale * residual;
        f += 0.5 * scale * residual * residual;
    }
    f
}

#[test]
fn resumable_and_callback_match_every_accepted_point_and_report() {
    for m in [2, 10, 20] {
        let options = Options::new()
            .with_history_size(m)
            .unwrap()
            .with_projected_gradient_tolerance(1e-10)
            .unwrap();
        let n = 64;
        let lo = vec![-0.5; n];
        let hi = vec![0.5; n];
        let bounds = Bounds::new(&lo, &hi, n).unwrap();
        let mut expected = vec![2.0; n];
        let mut trace = Vec::new();
        let reference = Solver::new(options)
            .minimize_with_callback(
                &mut expected,
                bounds,
                |x, g| Ok::<_, Infallible>(objective(x, g)),
                |it| {
                    trace.push((
                        it.x.to_vec(),
                        it.gradient.to_vec(),
                        it.value,
                        it.step_length,
                        it.step_norm(),
                        it.stats,
                    ));
                    Control::Continue
                },
            )
            .unwrap();
        let mut actual = vec![2.0; n];
        let mut solver = Solver::new(options);
        let mut run = solver.start(&mut actual, bounds).unwrap();
        let mut accepted = 0;
        let mut evaluations = 0;
        let report = loop {
            match run.state() {
                RunState::Evaluate(e) => {
                    evaluations += 1;
                    assert_eq!(e.context.evaluation, evaluations);
                    assert_eq!(e.context.accepted_iterations, accepted);
                    assert_eq!(
                        e.context.kind,
                        if evaluations == 1 {
                            EvaluationKind::Initial
                        } else {
                            EvaluationKind::Trial
                        }
                    );
                    assert!(e.gradient.iter().all(|g| g.is_nan()));
                    let value = objective(e.x, e.gradient);
                    run.submit(value).unwrap();
                }
                RunState::Accepted(it) => {
                    let expected = &trace[accepted];
                    assert_eq!(it.x, expected.0);
                    assert_eq!(it.gradient, expected.1);
                    assert_eq!(
                        (it.value, it.step_length, it.step_norm(), it.stats),
                        (expected.2, expected.3, expected.4, expected.5)
                    );
                    accepted += 1;
                    run.advance(Control::Continue).unwrap();
                }
                RunState::Finished(report) => break report,
                _ => unreachable!(),
            }
        };
        assert_eq!(accepted, trace.len());
        assert_eq!(report, reference);
        assert!(matches!(run.submit(0.0), Err(RunError::ExpectedEvaluation)));
        assert!(matches!(
            run.advance(Control::Continue),
            Err(RunError::ExpectedAccepted)
        ));
        assert!(matches!(run.state(),RunState::Finished(r) if r == report));
        drop(run);
        assert_eq!(actual, expected);
    }
}

#[test]
fn invalid_submission_is_repairable_and_drop_restores_pending_trial() {
    let mut solver = Solver::default();
    let mut x = [0.25, 0.25];
    let bounds = Bounds::new(&[-1.0; 2], &[1.0; 2], 2).unwrap();
    let mut run = solver.start(&mut x, bounds).unwrap();
    assert!(matches!(
        run.advance(Control::Continue),
        Err(RunError::ExpectedAccepted)
    ));
    assert!(matches!(
        run.submit(f64::NAN),
        Err(RunError::Evaluation(EvaluationError::NonFiniteValue))
    ));
    assert!(matches!(
        run.submit(0.0),
        Err(RunError::Evaluation(EvaluationError::NonFiniteGradient {
            index: 0
        }))
    ));
    let value = match run.state() {
        RunState::Evaluate(e) => objective(e.x, e.gradient),
        _ => unreachable!(),
    };
    run.submit(value).unwrap();
    match run.state() {
        RunState::Evaluate(e) => {
            assert_eq!(e.context.evaluation, 2);
            assert_ne!(e.x, &[0.25; 2]);
        }
        _ => unreachable!(),
    }
    run.abort();
    assert_eq!(x, [0.25; 2]);
    assert!(solver.workspace().gradient().is_empty());
}

#[test]
fn invalid_trial_does_not_commit_gradient_or_consume_request() {
    let mut solver = Solver::default();
    let bounds = Bounds::new(&[-1.0; 2], &[1.0; 2], 2).unwrap();
    let mut x = [0.25; 2];
    solver
        .minimize(&mut x, bounds, |x, g| Ok::<_, Infallible>(objective(x, g)))
        .unwrap();
    let previous = solver.workspace().gradient().to_vec();
    x.fill(0.25);
    let mut run = solver.start(&mut x, bounds).unwrap();
    let value = match run.state() {
        RunState::Evaluate(e) => objective(e.x, e.gradient),
        _ => unreachable!(),
    };
    run.submit(value).unwrap();
    assert!(matches!(run.submit(0.0), Err(RunError::Evaluation(_))));
    assert!(matches!(run.state(),RunState::Evaluate(e) if e.context.evaluation == 2));
    drop(run);
    assert_eq!(x, [0.25; 2]);
    assert_eq!(solver.workspace().gradient(), previous);
}

#[test]
fn evaluation_budget_and_user_stop_are_distinct() {
    for budget in [1, 2, 3] {
        let options = Options::new()
            .with_max_evaluations(budget)
            .unwrap()
            .with_relative_function_tolerance(0.0)
            .unwrap()
            .with_projected_gradient_tolerance(0.0)
            .unwrap();
        let mut solver = Solver::new(options);
        let mut x = [0.25; 2];
        let bounds = Bounds::new(&[-1.0; 2], &[1.0; 2], 2).unwrap();
        let report = solver
            .minimize(&mut x, bounds, |x, g| Ok::<_, Infallible>(objective(x, g)))
            .unwrap();
        assert_eq!(report.stats.evaluations, budget);
        assert_eq!(
            report.termination,
            Termination::Stopped(StopReason::MaximumEvaluations)
        );
    }
    let mut solver = Solver::default();
    let mut x = [0.25; 2];
    let bounds = Bounds::new(&[-1.0; 2], &[1.0; 2], 2).unwrap();
    let report = solver
        .minimize_with_callback(
            &mut x,
            bounds,
            |x, g| Ok::<_, Infallible>(objective(x, g)),
            |_| Control::Stop,
        )
        .unwrap();
    assert_eq!(report.termination, Termination::Stopped(StopReason::User));
}

#[test]
fn initially_stationary_point_finishes_without_accepted_event() {
    let mut solver = Solver::default();
    let mut x = [0.0; 2];
    let bounds = Bounds::new(&[-1.0; 2], &[1.0; 2], 2).unwrap();
    let mut run = solver.start(&mut x, bounds).unwrap();
    if let RunState::Evaluate(e) = run.state() {
        e.gradient.fill(0.0);
    }
    run.submit(0.0).unwrap();
    assert!(
        matches!(run.state(),RunState::Finished(r) if r.stats.iterations == 0 && r.stats.evaluations == 1)
    );
}

#[test]
fn context_adapter_receives_trial_metadata_without_breaking_legacy_callback() {
    struct Adapter {
        requests: usize,
    }
    impl SolveAdapter for Adapter {
        type Error = Infallible;
        fn value_and_gradient(&mut self, _: &[f64], _: &mut [f64]) -> Result<f64, Self::Error> {
            panic!("context hook expected")
        }
        fn value_and_gradient_with_context(
            &mut self,
            x: &[f64],
            g: &mut [f64],
            c: alloy_lbfgsb::EvaluationContext,
        ) -> Result<f64, Self::Error> {
            self.requests += 1;
            assert_eq!(c.evaluation, self.requests);
            Ok(objective(x, g))
        }
    }
    let mut solver = Solver::default();
    let mut x = [0.25; 2];
    let mut adapter = Adapter { requests: 0 };
    let bounds = Bounds::new(&[-1.0; 2], &[1.0; 2], 2).unwrap();
    let report = solver
        .minimize_with_adapter(&mut x, bounds, &mut adapter)
        .unwrap();
    assert_eq!(report.stats.evaluations, adapter.requests);
}
#[test]
fn cancellation_after_progress_restores_last_accepted_point() {
    let mut solver = Solver::default();
    let mut x = [0.25; 8];
    let bounds = Bounds::new(&[-1.0; 8], &[1.0; 8], 8).unwrap();
    let mut run = solver.start(&mut x, bounds).unwrap();
    let mut previous = [0.25; 8];
    loop {
        match run.state() {
            RunState::Evaluate(e) => {
                let value = objective(e.x, e.gradient);
                run.submit(value).unwrap();
            }
            RunState::Accepted(it) => {
                let mut norm: f64 = 0.0;
                for (&current, &old) in it.x.iter().zip(&previous) {
                    norm = norm.max((current - old).abs());
                }
                assert_eq!(it.step_norm(), norm);
                previous.copy_from_slice(it.x);
                run.advance(Control::Continue).unwrap();
                assert!(matches!(run.state(), RunState::Evaluate(_)));
                break;
            }
            _ => panic!("expected progress before termination"),
        }
    }
    drop(run);
    assert_eq!(x, previous);
}
