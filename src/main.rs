mod cli;
mod detect;
mod features;
mod image;
mod raid;
mod report;

use std::process::ExitCode;

fn main() -> ExitCode {
    match cli::run(std::env::args().collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}
