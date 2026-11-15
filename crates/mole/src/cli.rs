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
  .cube, .cub   Gaussian cube: atoms and an isosurface (red +, blue -)

Options:
  --list              print the structure table (tab-separated) and exit without opening a window
  --frame N           start at structure N (1 = first)
  --smooth            antialiased drawing with XRender
  --light             white theme
  --absolute          plot absolute energies instead of E - Emin
  --fix-orientation   turn every structure to the orientation of the first one shown
  --tol T             bond tolerance, 0.80 to 1.60 (default 1.25)
  --no-mm             do not draw metal-metal bonds
  --iso V             isovalue of cube surfaces (lobes at +V and -V)
  --transparent       translucent cube surfaces (turns on --smooth)
  --no-forces         do not draw force arrows
  -h, --help          show this help
  -V, --version       show the version

Every option is also a control in the window.

Keyboard: ← → previous/next · space play/pause · Home/End first/last · q/Esc quit

Exit codes: 0 ok · 1 could not open the window · 2 wrong usage ·
3 read error · 4 invalid file · 5 unsupported format";

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
    out += "   Formats: XYZ (incl. ORCA *_trj.xyz), Gaussian 16 and ORCA 6.1 outputs, cube\n\n";
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

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Options {
    pub smooth: bool,
    pub light: bool,
    pub absolute: bool,
    pub fix_orientation: bool,
    pub no_mm: bool,
    pub transparent: bool,
    pub no_forces: bool,
    pub frame: Option<usize>,
    pub tol: Option<f64>,
    pub iso: Option<f64>,
}

fn number(v: Option<std::ffi::OsString>) -> Option<f64> {
    v?.to_str()?.parse::<f64>().ok().filter(|v| v.is_finite())
}

fn usage_error(msg: &str) -> ExitCode {
    eprintln!("mole: {msg}\n\n{USAGE}");
    ExitCode::from(2)
}

pub fn main(open: impl FnOnce(Loaded, &str, Options) -> Result<(), String>) -> ExitCode {
    let mut list = false;
    let mut options = Options::default();
    let mut files = Vec::new();
    let mut only_files = false;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
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
            Some("--light") => options.light = true,
            Some("--absolute") => options.absolute = true,
            Some("--fix-orientation") => options.fix_orientation = true,
            Some("--no-mm") => options.no_mm = true,
            Some("--transparent") => options.transparent = true,
            Some("--no-forces") => options.no_forces = true,
            Some("--frame") => {
                match number(args.next()).filter(|n| *n >= 1.0 && n.fract() == 0.0) {
                    Some(n) => options.frame = Some(n as usize),
                    None => return usage_error("--frame needs a structure number (1, 2, ...)"),
                }
            }
            Some("--tol") => match number(args.next()).filter(|t| (0.8..=1.6).contains(t)) {
                Some(t) => options.tol = Some(t),
                None => return usage_error("--tol needs a number from 0.80 to 1.60"),
            },
            Some("--iso") => match number(args.next()).filter(|v| *v > 0.0) {
                Some(v) => options.iso = Some(v),
                None => return usage_error("--iso needs a positive number"),
            },
            Some(s) if s.starts_with('-') => return usage_error(&format!("unknown option '{s}'")),
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
