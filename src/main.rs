use std::process::ExitCode;

fn main() -> ExitCode {
    match agents_sessions::commands::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
