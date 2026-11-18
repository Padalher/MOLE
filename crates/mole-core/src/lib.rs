#![forbid(unsafe_code)]

pub mod cube;
pub mod elements;
pub mod gaussian;
pub mod orca;
pub mod xyz;

use std::fmt;
use std::path::{Path, PathBuf};

pub const MAX_FILE_BYTES: u64 = 1 << 30;

#[derive(Debug, Clone, PartialEq)]
pub struct Atom {
    pub z: u8,
    pub pos: [f64; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnergySource {
    XyzComment,
    GaussianScf,
    OrcaFinal,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Structure {
    pub atoms: Vec<Atom>,
    pub energy: Option<f64>,
    pub energy_source: Option<EnergySource>,
    pub label: String,
    pub comment: String,
    pub path: PathBuf,
    pub line: usize,
    pub columns: Vec<Column>,
    pub grid: Option<Box<cube::Grid>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    pub name: String,
    pub width: usize,
    pub values: Vec<f64>,
}

impl Structure {
    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|c| c.name == name)
    }

    pub fn formula(&self) -> String {
        let mut counts = [0usize; 119];
        for a in &self.atoms {
            counts[usize::from(a.z)] += 1;
        }
        let mut syms: Vec<(&str, usize)> = (1..=118u8)
            .filter(|&z| counts[usize::from(z)] > 0)
            .map(|z| (elements::symbol(z), counts[usize::from(z)]))
            .collect();
        let has_c = counts[6] > 0;
        syms.sort_by_key(|&(s, _)| match (has_c, s) {
            (true, "C") => (0, s),
            (true, "H") => (1, s),
            _ => (2, s),
        });
        syms.iter()
            .map(|&(s, n)| {
                if n == 1 {
                    s.to_string()
                } else {
                    format!("{s}{n}")
                }
            })
            .collect()
    }
}

pub const DEFAULT_BOND_TOLERANCE: f64 = 1.25;

pub const MAX_BONDED_ATOMS: usize = 10_000;

pub fn bonds(atoms: &[Atom], tolerance: f64, metal_metal: bool) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    if atoms.len() > MAX_BONDED_ATOMS {
        return out;
    }
    for (i, a) in atoms.iter().enumerate() {
        for (j, b) in atoms.iter().enumerate().skip(i + 1) {
            if !metal_metal && elements::is_metal(a.z) && elements::is_metal(b.z) {
                continue;
            }
            let d2: f64 = (0..3).map(|k| (a.pos[k] - b.pos[k]).powi(2)).sum();
            let max = tolerance * (elements::covalent_radius(a.z) + elements::covalent_radius(b.z));
            if d2 < max * max {
                out.push((i, j));
            }
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Io,
    Parse,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    pub kind: ErrorKind,
    pub path: PathBuf,
    pub line: Option<usize>,
    pub msg: String,
}

impl Error {
    fn parse(path: &Path, line: usize, msg: impl Into<String>) -> Self {
        Error {
            kind: ErrorKind::Parse,
            path: path.to_path_buf(),
            line: Some(line),
            msg: msg.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.line {
            Some(l) => write!(f, "{}:{}: {}", self.path.display(), l, self.msg),
            None => write!(f, "{}: {}", self.path.display(), self.msg),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Default)]
pub struct Loaded {
    pub structures: Vec<Structure>,
    pub warnings: Vec<String>,
    pub errors: Vec<Error>,
}

pub fn read_file(path: &Path) -> Result<String, Error> {
    let io = |msg: String| Error {
        kind: ErrorKind::Io,
        path: path.to_path_buf(),
        line: None,
        msg,
    };
    let meta = std::fs::metadata(path).map_err(|e| io(e.to_string()))?;
    if !meta.is_file() {
        return Err(io("not a regular file".into()));
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(io(format!(
            "file too large ({} bytes; maximum {MAX_FILE_BYTES})",
            meta.len()
        )));
    }
    let bytes = std::fs::read(path).map_err(|e| io(e.to_string()))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn add_step(
    steps: &mut Vec<Structure>,
    atoms: Vec<Atom>,
    energy: Option<f64>,
    source: EnergySource,
    path: &Path,
    start: usize,
) -> Result<(), Error> {
    if atoms.is_empty() {
        return Err(Error::parse(path, start + 1, "geometry without atoms"));
    }
    if !steps
        .last()
        .is_some_and(|p| p.atoms == atoms && p.energy == energy)
    {
        let program = if source == EnergySource::OrcaFinal {
            "ORCA"
        } else {
            "Gaussian"
        };
        steps.push(Structure {
            atoms,
            energy,
            energy_source: energy.map(|_| source),
            comment: format!("{program}: {}", path.display()),
            label: String::new(),
            path: path.to_path_buf(),
            line: start + 1,
            columns: Vec::new(),
            grid: None,
        });
    }
    Ok(())
}

fn name_steps(steps: &mut [Structure], path: &Path) {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let many = steps.len() > 1;
    for (k, s) in steps.iter_mut().enumerate() {
        s.label = if many {
            format!("{stem} {}", k + 1)
        } else {
            stem.to_string()
        };
    }
}

fn is_output_ext(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("out") || e.eq_ignore_ascii_case("log"))
}

pub fn parse_output(text: &str, path: &Path) -> Result<(Vec<Structure>, Vec<String>), Error> {
    if orca::is_orca(text) {
        orca::parse(text, path)
    } else {
        gaussian::parse(text, path)
    }
}

fn load_one(path: &Path, out: &mut Loaded) -> Result<(), Error> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext.eq_ignore_ascii_case("xyz") {
        let text = read_file(path)?;
        let mut frames = xyz::parse(&text, path)?;
        fill_energies_from_outputs(path, &mut frames, &mut out.warnings);
        out.structures.extend(frames);
    } else if ext.eq_ignore_ascii_case("cube") || ext.eq_ignore_ascii_case("cub") {
        let (s, warnings) = cube::parse(&read_file(path)?, path)?;
        out.warnings.extend(warnings);
        out.structures.push(s);
    } else if is_output_ext(path) {
        let text = read_file(path)?;
        let (s, warnings) = parse_output(&text, path)?;
        out.warnings.extend(warnings);
        out.structures.extend(s);
    } else {
        return Err(Error {
            kind: ErrorKind::Unsupported,
            path: path.to_path_buf(),
            line: None,
            msg: "unsupported extension (accepted: .xyz, .cube/.cub, and .out/.log from Gaussian or ORCA)"
                .into(),
        });
    }
    Ok(())
}

pub fn load(paths: &[PathBuf]) -> Loaded {
    let mut out = Loaded::default();
    for path in paths {
        if let Err(e) = load_one(path, &mut out) {
            out.warnings.push(format!("file skipped: {e}"));
            out.errors.push(e);
        }
    }
    let plain = out.structures.iter().filter(|s| s.grid.is_none());
    let missing = plain.filter(|s| s.energy.is_none()).count();
    if missing > 0 {
        out.warnings.push(format!(
            "{missing} of {} structures have no energy: they can be browsed but are not plotted",
            out.structures.len()
        ));
    }
    let has = |src| out.structures.iter().any(|s| s.energy_source == Some(src));
    if has(EnergySource::XyzComment)
        && (has(EnergySource::GaussianScf) || has(EnergySource::OrcaFinal))
    {
        out.warnings.push(
            "the plot mixes energies from XYZ comments and from Gaussian/ORCA (Hartree): units may differ"
                .into(),
        );
    }
    out
}

fn fill_energies_from_outputs(
    xyz_path: &Path,
    frames: &mut [Structure],
    warnings: &mut Vec<String>,
) {
    if frames.iter().any(|f| f.energy.is_some()) || frames.iter().all(|f| f.label.is_empty()) {
        return;
    }
    let dir = xyz_path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let outputs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| is_output_ext(p))
        .collect();
    let mut read: Vec<Option<(Option<f64>, Option<EnergySource>)>> = vec![None; outputs.len()];
    for f in frames.iter_mut().filter(|f| !f.label.is_empty()) {
        let suffix = format!("_{}", f.label);
        let Some(k) = outputs.iter().position(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s == f.label || s.ends_with(&suffix))
        }) else {
            continue;
        };
        let (energy, source) = *read[k].get_or_insert_with(|| {
            match read_file(&outputs[k]).and_then(|t| parse_output(&t, &outputs[k])) {
                Ok((s, _)) => s
                    .last()
                    .map_or((None, None), |s| (s.energy, s.energy_source)),
                Err(e) => {
                    warnings.push(format!("could not read the associated energy: {e}"));
                    (None, None)
                }
            }
        });
        if energy.is_some() {
            f.energy = energy;
            f.energy_source = source;
        }
    }
}

#[cfg(test)]
fn mutations(base: &str, n: usize, mut f: impl FnMut(&str)) {
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for _ in 0..n {
        let mut buf = base.as_bytes().to_vec();
        for _ in 0..(next() % 4 + 1) {
            let i = (next() as usize) % buf.len();
            match next() % 3 {
                0 => buf[i] = (next() % 256) as u8,
                1 => {
                    buf.remove(i);
                }
                _ => buf.insert(i, b"0123456789\n -.eE"[(next() % 16) as usize]),
            }
            if buf.is_empty() {
                break;
            }
        }
        f(&String::from_utf8_lossy(&buf));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn huge_structures_have_no_bonds() {
        let atoms: Vec<Atom> = (0..=MAX_BONDED_ATOMS).map(|k| atom(6, k as f64)).collect();
        assert!(bonds(&atoms, 1.25, true).is_empty());
        assert_eq!(bonds(&atoms[..3], 1.25, true), vec![(0, 1), (1, 2)]);
    }

    #[test]
    fn only_regular_files_are_read() {
        let e = read_file(&std::env::temp_dir()).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Io);
        assert!(e.msg.contains("not a regular file"), "{}", e.msg);
    }

    fn atom(z: u8, x: f64) -> Atom {
        Atom {
            z,
            pos: [x, 0.0, 0.0],
        }
    }

    #[test]
    fn bonds_by_covalent_radii() {
        let atoms = [atom(8, 0.0), atom(1, 0.96), atom(1, -0.96)];
        assert_eq!(bonds(&atoms, 1.15, false), vec![(0, 1), (0, 2)]);
        assert!(bonds(&atoms, 0.5, false).is_empty());
        let ti = [atom(22, 0.0), atom(22, 2.7)];
        assert!(bonds(&ti, DEFAULT_BOND_TOLERANCE, false).is_empty());
        assert_eq!(bonds(&ti, DEFAULT_BOND_TOLERANCE, true), vec![(0, 1)]);
    }

    #[test]
    fn hill_formula() {
        let s = |zs: &[u8]| Structure {
            atoms: zs.iter().map(|&z| atom(z, 0.0)).collect(),
            energy: None,
            energy_source: None,
            label: String::new(),
            comment: String::new(),
            path: PathBuf::new(),
            line: 1,
            columns: Vec::new(),
            grid: None,
        };
        assert_eq!(s(&[8, 1, 1]).formula(), "H2O");
        assert_eq!(s(&[8, 6, 1, 1, 1, 1]).formula(), "CH4O");
        assert_eq!(s(&[22, 8, 8, 13]).formula(), "AlO2Ti");
    }

    #[test]
    fn unsupported_extension_is_reported_not_fatal() {
        let l = load(&[PathBuf::from("no-existe.pdb")]);
        assert!(l.structures.is_empty());
        assert_eq!(l.errors[0].kind, ErrorKind::Unsupported);
        let l = load(&[PathBuf::from("no-existe.xyz")]);
        assert_eq!(l.errors[0].kind, ErrorKind::Io);
    }
}
