use std::env;
use std::process;
use syzaster::pkg::kcov::run_check_asterinas_kcov;

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args
        .first()
        .map(|s| s.as_str())
        .unwrap_or("check-asterinas-kcov");
    if let Err(code) = run_check_asterinas_kcov(prog, &args[1..]) {
        process::exit(code);
    }
}
