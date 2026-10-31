use std::env;
use std::process;
use syzaster::pkg::eval::run_eval_fuzz;

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| s.as_str()).unwrap_or("eval-fuzz");
    if let Err(code) = run_eval_fuzz(prog, &args[1..]) {
        process::exit(code);
    }
}
