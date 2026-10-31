use std::env;
use std::process;
use syzaster::pkg::kcov::run_check_kcov_instrumentation;

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args
        .first()
        .map(|s| s.as_str())
        .unwrap_or("check-kcov-inst");
    if let Err(code) = run_check_kcov_instrumentation(prog, &args[1..]) {
        process::exit(code);
    }
}
