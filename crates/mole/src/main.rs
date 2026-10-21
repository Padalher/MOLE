//! `mole <archivo>...`: visor con el protocolo X11 básico, fluido por `ssh -X/-Y`.
//! `mole --list` imprime la tabla de estructuras sin abrir ventana.

mod app;
mod cli;
mod x11;

fn main() -> std::process::ExitCode {
    cli::main(x11::run)
}
