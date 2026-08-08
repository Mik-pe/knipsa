//! Common warm-up, calibration, and sampling protocol for Rust references.

use std::{hint::black_box, time::Instant};

pub const WARMUP_RUNS: usize = 3;
pub const SAMPLE_RUNS: usize = 25;
pub const MIN_SAMPLE_TIME_NS: u128 = 2_000_000;
const MAX_ITERATIONS_PER_SAMPLE: usize = 1 << 20;

pub struct Measurement<Output> {
    pub output: Output,
    pub median_ns: u128,
    pub p95_ns: u128,
    pub iterations_per_sample: usize,
}

pub fn measure<Output, Error, Run>(mut run: Run) -> Result<Measurement<Output>, Error>
where
    Run: FnMut() -> Result<Output, Error>,
{
    let mut output = black_box(run()?);
    for _ in 1..WARMUP_RUNS {
        output = black_box(run()?);
    }

    let mut iterations_per_sample = 1;
    loop {
        let started = Instant::now();
        for _ in 0..iterations_per_sample {
            output = black_box(run()?);
        }
        if started.elapsed().as_nanos() >= MIN_SAMPLE_TIME_NS
            || iterations_per_sample == MAX_ITERATIONS_PER_SAMPLE
        {
            break;
        }
        iterations_per_sample = (iterations_per_sample * 2).min(MAX_ITERATIONS_PER_SAMPLE);
    }

    let mut timings = Vec::with_capacity(SAMPLE_RUNS);
    for _ in 0..SAMPLE_RUNS {
        let started = Instant::now();
        for _ in 0..iterations_per_sample {
            output = black_box(run()?);
        }
        timings.push(started.elapsed().as_nanos() / iterations_per_sample as u128);
    }
    timings.sort_unstable();

    Ok(Measurement {
        output,
        median_ns: timings[timings.len() / 2],
        p95_ns: timings[(timings.len() * 95).div_ceil(100) - 1],
        iterations_per_sample,
    })
}
