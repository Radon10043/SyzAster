use std::env;
use std::process;
use syzaster::pkg::{eval, kcov};

fn print_help(prog: &str) {
    println!(
        "SyzAster: Coverage-Guided Kernel Fuzzing Prototype Tool for Asterinas Framekernel

Usage: {prog} <SUBCOMMAND> [ARGS...]

Subcommands:
  check-kcov              Build and run KCOV ABI test inside a booted Asterinas VM
  check-inst              Verify Rust compiler and KCOV wrapper instrumentation
  kcov-rustc              Rust compiler wrapper for KCOV=1 builds
  eval-fuzz               Evaluate fuzzing campaign coverage, crashes, A12, and p-values
  help                    Show this help message"
    );
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| s.as_str()).unwrap_or("syzaster");
    let Some(sub) = args.get(1) else {
        print_help(prog);
        return;
    };
    let sub_args = &args[2..];

    let res = match sub.as_str() {
        "check-kcov" | "check-asterinas-kcov" => {
            kcov::run_check_asterinas_kcov("syzaster check-kcov", sub_args)
        }
        "check-inst" | "check-kcov-inst" => {
            kcov::run_check_kcov_instrumentation("syzaster check-inst", sub_args)
        }
        "kcov-rustc" => kcov::run_kcov_rustc(sub_args),
        "eval" | "eval-fuzz" => eval::run_eval_fuzz("syzaster eval-fuzz", sub_args),
        "-h" | "--help" | "help" => {
            print_help(prog);
            Ok(())
        }
        other => {
            eprintln!("unknown subcommand: {}", other);
            print_help(prog);
            Err(1)
        }
    };

    if let Err(code) = res {
        process::exit(code);
    }
}
