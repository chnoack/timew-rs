use std::io::Write;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() == 1 && args[0] == "--version" {
        println!("{}", timew_cli::VERSION);
        return ExitCode::SUCCESS;
    }
    let outcome = timew_cli::run(&args);
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(outcome.stdout.as_bytes());
    let _ = out.flush();
    eprint!("{}", outcome.stderr);
    ExitCode::from((outcome.status & 0xFF) as u8)
}
