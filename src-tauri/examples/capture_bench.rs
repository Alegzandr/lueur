//! What the ambilight costs this machine: capture plus colour computation per
//! frame, and the CPU the process uses at the target rate. No bulb needed.
//!
//! ```text
//! cargo run --release -p lueur --example capture_bench -- [options]
//!   --backend NAME   auto (what the ambilight uses), gdi, xcap, or all (default)
//!   --frames N       frames measured per backend (default 300)
//!   --fps N          target rate (default 45, the app's default)
//!   --monitor N      display index, as in Settings (default 0)
//!   --lights N       lights to compute colours for (default 8)
//! ```
//!
//! Something should move on screen (a video, a game) while it runs: a still
//! desktop sends Desktop Duplication and PipeWire nothing new to read.

use lueur_lib::capture_bench::{Report, Setup, backends, run};

fn main() {
    let mut setup = Setup::default();
    let mut backend = String::from("all");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || {
            args.next()
                .unwrap_or_else(|| usage(&format!("{arg} needs a value")))
        };
        match arg.as_str() {
            "--backend" => backend = value(),
            "--frames" => setup.frames = number(&value()),
            "--fps" => setup.fps = number(&value()),
            "--monitor" => setup.monitor = number(&value()),
            "--lights" => setup.lights = number(&value()),
            "-h" | "--help" => usage(""),
            other => usage(&format!("unknown option {other}")),
        }
    }
    setup.lights = setup.lights.max(1);
    setup.frames = setup.frames.max(1);

    let chosen: Vec<&'static str> = match backends().iter().find(|b| **b == backend) {
        Some(b) => vec![*b],
        None if backend == "all" => backends().to_vec(),
        None => usage(&format!("unknown backend {backend}")),
    };

    println!(
        "{} frames at {} fps, {} lights, display {}",
        setup.frames, setup.fps, setup.lights, setup.monitor
    );
    println!(
        "{:<6} {:>11} {:>16} {:>16} {:>11}",
        "path", "frame", "capture avg/p95", "compute avg/p95", "CPU (1 core)"
    );
    for name in chosen {
        match run(name, setup) {
            Ok(report) => print_row(&report),
            Err(e) => println!("{name:<6} unavailable: {e}"),
        }
    }
}

fn print_row(r: &Report) {
    let cpu = r
        .cpu_percent
        .map_or_else(|| "n/a".to_string(), |c| format!("{c:.1} %"));
    println!(
        "{:<6} {:>11} {:>7.3}/{:<8.3} {:>7.3}/{:<8.3} {:>11}{}",
        r.backend,
        format!("{}x{}", r.width, r.height),
        r.capture.mean_ms,
        r.capture.p95_ms,
        r.compute.mean_ms,
        r.compute.p95_ms,
        cpu,
        if r.errors > 0 {
            format!("  ({} failed grabs)", r.errors)
        } else {
            String::new()
        }
    );
}

fn number<T: std::str::FromStr>(s: &str) -> T {
    s.parse()
        .unwrap_or_else(|_| usage(&format!("{s} is not a number")))
}

fn usage(error: &str) -> ! {
    if !error.is_empty() {
        eprintln!("capture_bench: {error}");
    }
    eprintln!(
        "usage: capture_bench [--backend auto|gdi|xcap|all] [--frames N] [--fps N] \
         [--monitor N] [--lights N]"
    );
    std::process::exit(if error.is_empty() { 0 } else { 2 });
}
