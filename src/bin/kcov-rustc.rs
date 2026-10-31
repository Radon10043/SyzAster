use std::env;
use std::process;
use syzaster::pkg::kcov::run_kcov_rustc;

fn main() {
    let args: Vec<String> = env::args().collect();
    if let Err(code) = run_kcov_rustc(&args[1..]) {
        process::exit(code);
    }
}
