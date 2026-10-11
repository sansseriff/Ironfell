//! Native timing of the phase 1 benchmark: a 12-variable chain at 20
//! iterations. `cargo run -p iron_solve --release --example bench --target
//! aarch64-apple-darwin`. The same problem runs in wasm through the app's
//! `solve_bench` FFI (`__iron.bench()` in the shell).

use std::time::Instant;

fn main() {
    let (iterations, repeats) = (20, 200);
    // Warm up once so page faults and allocation are not in the measurement.
    iron_solve::bench::run(iterations, 1);
    let t0 = Instant::now();
    let s = iron_solve::bench::run(iterations, repeats);
    let per_solve = t0.elapsed().as_secs_f64() * 1e6 / repeats as f64;
    println!(
        "12-var chain, {} of {iterations} iterations: {per_solve:.1} µs per solve ({} evaluations); required {:.2e}, strong {:.3}",
        s.iterations, s.evaluations, s.max_required_residual, s.max_strong_residual
    );
}
