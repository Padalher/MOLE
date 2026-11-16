#![forbid(unsafe_code)]

mod app;
mod cli;
mod x11;

fn main() -> std::process::ExitCode {
    cli::main(x11::run)
}
