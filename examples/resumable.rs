use alloy_lbfgsb::{Bounds, Control, Options, RunState, Solver};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut x = [3.0, -4.0];
    let bounds = Bounds::new(&[-10.0; 2], &[10.0; 2], 2)?;
    // Disable function stagnation as a sufficient convergence condition.
    let options = Options::new().with_relative_function_tolerance(0.0)?;
    let mut solver = Solver::new(options);
    let mut run = solver.start(&mut x, bounds)?;
    loop {
        match run.state() {
            RunState::Evaluate(e) => {
                // An application may suspend here to schedule an external evaluation.
                e.gradient[0] = 2.0 * (e.x[0] - 1.0);
                e.gradient[1] = 2.0 * (e.x[1] + 2.0);
                let value = (e.x[0] - 1.0).powi(2) + (e.x[1] + 2.0).powi(2);
                run.submit(value)?;
            }
            RunState::Accepted(it) => {
                println!(
                    "iteration={}, value={}, projected_gradient={}, step={}",
                    it.stats.iterations, it.value, it.projected_gradient_norm, it.step_length
                );
                run.advance(Control::Continue)?;
            }
            RunState::Finished(report) => {
                println!("{:?}", report.termination);
                break;
            }
            _ => unreachable!("unrecognized run state"),
        }
    }
    Ok(())
}
