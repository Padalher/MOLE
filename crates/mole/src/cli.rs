//! Línea de comandos de `mole`: ayuda, banner, `--list` y códigos de salida.

use mole_core::{ErrorKind, Loaded, Structure, load};
use std::path::PathBuf;
use std::process::ExitCode;

pub const USAGE: &str = "usage: mole [options] <file>...

Opens a window with the 3D structure, the energy plot E vs n (click a point to select that
structure) and playback controls.

Files:
  .xyz          one or more structures; energy taken from the comment line
                (includes ORCA *_trj.xyz trajectories)
  .out, .log    Gaussian or ORCA output: final geometry and its energy

Options:
  --list        print the structure table (tab-separated) and exit without opening a window
  --smooth      antialiased drawing with XRender (also a checkbox in the window)
  -h, --help    show this help
  -V, --version show the version

Keyboard: ← → previous/next · space play/pause · Home/End first/last · q/Esc quit

Exit codes: 0 ok · 1 could not open the window · 2 wrong usage ·
3 read error · 4 invalid file · 5 unsupported format";

/// Banner al ejecutar `mole` sin argumentos. En color solo si la salida es una terminal y no se
/// definió `NO_COLOR`.
pub fn banner() -> String {
    use std::io::IsTerminal;
    const LOGO: [&str; 4] = [
        "██▄  ▄██   ▄███▄   ██       ██████",
        "██ ▀▀ ██  ██   ██  ██       ██▄▄  ",
        "██    ██  ██   ██  ██       ██▀▀  ",
        "██    ██   ▀███▀   ███████  ██████",
    ];
    let color = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let paint = |code: &str, text: &str| {
        if color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    };
    let mut out = String::from("\n");
    for (row, code) in LOGO
        .iter()
        .zip(["1;38;5;51", "1;38;5;45", "1;38;5;39", "1;38;5;33"])
    {
        out += &format!("   {}\n", paint(code, row));
    }
    out += &format!(
        "\n   {} {}  ·  molecular structure viewer with an E vs n energy plot\n",
        paint("1", "MOLE"),
        paint("1;38;5;45", env!("CARGO_PKG_VERSION"))
    );
    out += &format!(
        "   Authors: {}\n",
        env!("CARGO_PKG_AUTHORS").replace(':', ", ")
    );
    out += "   Formats: XYZ (incl. ORCA *_trj.xyz), Gaussian 16 and ORCA 6.1 outputs\n\n";
    out += &format!(
        "   {}  mole <file>...          open the viewer\n",
        paint("1", "Usage:")
    );
    out += "           mole --list <file>...   print the structure table\n";
    out += "           mole --help             all options\n";
    out
}

pub fn exit_code(kind: ErrorKind) -> ExitCode {
    ExitCode::from(match kind {
        ErrorKind::Io => 3,
        ErrorKind::Parse => 4,
        ErrorKind::Unsupported => 5,
    })
}

/// Opciones de la ventana que no son archivos.
#[derive(Debug, Default, Clone, Copy)]
pub struct Options {
    /// `--smooth`: dibujo suavizado con XRender.
    pub smooth: bool,
}

/// Punto de entrada común: interpreta los argumentos, carga los archivos y, si hay que abrir la
/// ventana, llama a `open` con las estructuras, el título y las opciones.
pub fn main(open: impl FnOnce(Loaded, &str, Options) -> Result<(), String>) -> ExitCode {
    let mut list = false;
    let mut options = Options::default();
    let mut files = Vec::new();
    let mut only_files = false;
    for arg in std::env::args_os().skip(1) {
        match arg.to_str() {
            _ if only_files => files.push(PathBuf::from(arg)),
            Some("--") => only_files = true,
            Some("-h" | "--help") => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            Some("-V" | "--version") => {
                println!("mole {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            Some("--list") => list = true,
            Some("--smooth") => options.smooth = true,
            Some(s) if s.starts_with('-') => {
                eprintln!("mole: unknown option '{s}'\n\n{USAGE}");
                return ExitCode::from(2);
            }
            _ => files.push(PathBuf::from(arg)),
        }
    }
    if files.is_empty() {
        if only_files || list {
            eprintln!("mole: at least one file is required\n\n{USAGE}");
            return ExitCode::from(2);
        }
        print!("{}", banner());
        return ExitCode::SUCCESS;
    }

    let loaded = load(&files);
    for w in &loaded.warnings {
        eprintln!("mole: warning: {w}");
    }
    if loaded.structures.is_empty() {
        eprintln!("mole: no structures were loaded");
        return exit_code(loaded.errors.first().map_or(ErrorKind::Parse, |e| e.kind));
    }

    if list {
        print_table(&loaded.structures);
        return loaded
            .errors
            .first()
            .map_or(ExitCode::SUCCESS, |e| exit_code(e.kind));
    }

    let title = format!(
        "MOLE — {}",
        files
            .iter()
            .map(|f| f.file_name().unwrap_or(f.as_os_str()).to_string_lossy())
            .collect::<Vec<_>>()
            .join(", ")
    );
    match open(loaded, &title, options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!(
                "mole: could not open the window: {e}\n\
                 Is there a display? Over SSH use 'ssh -X' (or -Y), or 'mole --list' for the table."
            );
            ExitCode::from(1)
        }
    }
}

pub fn print_table(structures: &[Structure]) {
    println!("n\tenergy\tlabel\tatoms\tformula\tsource");
    for (i, s) in structures.iter().enumerate() {
        let e = s.energy.map_or("-".to_string(), |e| e.to_string());
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}:{}",
            i + 1,
            e,
            if s.label.is_empty() { "-" } else { &s.label },
            s.atoms.len(),
            s.formula(),
            s.path.display(),
            s.line
        );
    }
}
