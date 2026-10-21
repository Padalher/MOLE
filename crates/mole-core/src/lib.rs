//! Lectura de estructuras y energías (XYZ, salidas de Gaussian y ORCA). Sin gráficos.
#![forbid(unsafe_code)]

pub mod elements;
pub mod gaussian;
pub mod orca;
pub mod xyz;

use std::fmt;
use std::path::{Path, PathBuf};

/// Tamaño máximo de archivo que se lee.
// ponytail: límite fijo; hacerlo configurable si aparecen salidas mayores.
pub const MAX_FILE_BYTES: u64 = 1 << 30;

#[derive(Debug, Clone, PartialEq)]
pub struct Atom {
    pub z: u8,
    /// Posición en Å.
    pub pos: [f64; 3],
}

/// De dónde salió la energía de una estructura.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnergySource {
    /// Línea de comentario de un XYZ (unidad desconocida; en GLOMOS, kcal/mol).
    XyzComment,
    /// Última `SCF Done` de una salida de Gaussian (Hartree).
    GaussianScf,
    /// Última `FINAL SINGLE POINT ENERGY` de una salida de ORCA (Hartree).
    OrcaFinal,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Structure {
    pub atoms: Vec<Atom>,
    /// Energía tal como se leyó; `None` si no consta (nunca se inventa).
    pub energy: Option<f64>,
    pub energy_source: Option<EnergySource>,
    /// Etiqueta (p. ej. `stage200001`, `3a` o el nombre del archivo).
    pub label: String,
    pub comment: String,
    pub path: PathBuf,
    /// Línea (1-based) donde empieza la estructura en `path`.
    pub line: usize,
}

impl Structure {
    /// Fórmula en orden de Hill (C, H y luego alfabético; alfabético si no hay C).
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

/// Tolerancia por defecto de [`bonds`]. En los óxidos Ti/Al de GLOMOS, los enlaces M–O quedan por
/// debajo de 1.2 × (r₁ + r₂) y los pares no enlazados por encima de 1.3.
pub const DEFAULT_BOND_TOLERANCE: f64 = 1.25;

/// Pares de átomos enlazados: distancia < `tolerance` × (r₁ + r₂), con radios covalentes.
/// Si `metal_metal` es falso se omiten los pares metal–metal: en óxidos, las distancias entre
/// cationes son tan cortas como los enlaces M–O y no se distinguen por distancia.
// ponytail: O(n²); usar una rejilla espacial si se ven estructuras de miles de átomos.
pub fn bonds(atoms: &[Atom], tolerance: f64, metal_metal: bool) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
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
    /// No se pudo leer el archivo (código de salida 3).
    Io,
    /// Contenido inválido (código de salida 4).
    Parse,
    /// Formato no soportado (código de salida 5).
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

/// Resultado de cargar varios archivos.
#[derive(Debug, Default)]
pub struct Loaded {
    pub structures: Vec<Structure>,
    /// Avisos para el usuario (datos ausentes, archivos omitidos, mezclas de unidades…).
    pub warnings: Vec<String>,
    /// Archivos que no se pudieron leer; el resto se carga igualmente.
    pub errors: Vec<Error>,
}

/// Lee un archivo completo respetando `MAX_FILE_BYTES`.
pub fn read_file(path: &Path) -> Result<String, Error> {
    let io = |msg: String| Error {
        kind: ErrorKind::Io,
        path: path.to_path_buf(),
        line: None,
        msg,
    };
    let len = std::fs::metadata(path)
        .map_err(|e| io(e.to_string()))?
        .len();
    if len > MAX_FILE_BYTES {
        return Err(io(format!(
            "file too large ({len} bytes; maximum {MAX_FILE_BYTES})"
        )));
    }
    let bytes = std::fs::read(path).map_err(|e| io(e.to_string()))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn is_output_ext(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("out") || e.eq_ignore_ascii_case("log"))
}

/// Salida de un programa de química cuántica: ORCA si lo indica la cabecera; si no, Gaussian.
pub fn parse_output(text: &str, path: &Path) -> Result<(Structure, Vec<String>), Error> {
    if orca::is_orca(text) {
        orca::parse(text, path)
    } else {
        gaussian::parse(text, path)
    }
}

/// Carga un archivo según su extensión: `.xyz` → XYZ; `.out`/`.log` → salida de Gaussian u ORCA.
fn load_one(path: &Path, out: &mut Loaded) -> Result<(), Error> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext.eq_ignore_ascii_case("xyz") {
        let text = read_file(path)?;
        let mut frames = xyz::parse(&text, path)?;
        fill_energies_from_outputs(path, &mut frames, &mut out.warnings);
        out.structures.extend(frames);
    } else if is_output_ext(path) {
        let text = read_file(path)?;
        let (s, warnings) = parse_output(&text, path)?;
        out.warnings.extend(warnings);
        out.structures.push(s);
    } else {
        return Err(Error {
            kind: ErrorKind::Unsupported,
            path: path.to_path_buf(),
            line: None,
            msg: "unsupported extension (accepted: .xyz, and .out/.log from Gaussian or ORCA)"
                .into(),
        });
    }
    Ok(())
}

/// Carga varios archivos en orden. Un archivo con errores se omite y se informa; el resto se carga.
pub fn load(paths: &[PathBuf]) -> Loaded {
    let mut out = Loaded::default();
    for path in paths {
        if let Err(e) = load_one(path, &mut out) {
            out.warnings.push(format!("file skipped: {e}"));
            out.errors.push(e);
        }
    }
    let missing = out.structures.iter().filter(|s| s.energy.is_none()).count();
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

/// Si **ninguna** estructura de un XYZ trae energía, la toma de la salida (Gaussian u ORCA) asociada a
/// cada etiqueta: un `.out`/`.log` del mismo directorio cuyo nombre sea `<etiqueta>` o termine en
/// `_<etiqueta>`. No se rellena si solo faltan algunas, para no mezclar unidades.
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
    for f in frames.iter_mut().filter(|f| !f.label.is_empty()) {
        let suffix = format!("_{}", f.label);
        let Some(out_path) = outputs.iter().find(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s == f.label || s.ends_with(&suffix))
        }) else {
            continue;
        };
        match read_file(out_path).and_then(|t| parse_output(&t, out_path)) {
            Ok((s, _)) if s.energy.is_some() => {
                f.energy = s.energy;
                f.energy_source = s.energy_source;
            }
            Ok(_) => {}
            Err(e) => warnings.push(format!("could not read the associated energy: {e}")),
        }
    }
}

/// Prueba de entrada hostil: aplica `n` mutaciones aleatorias (deterministas) a `base` y llama a
/// `f` con cada resultado; `f` no debe entrar en pánico.
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

    fn atom(z: u8, x: f64) -> Atom {
        Atom {
            z,
            pos: [x, 0.0, 0.0],
        }
    }

    #[test]
    fn bonds_by_covalent_radii() {
        // O–H a 0.96 Å enlazados; H–H a 1.9 Å no.
        let atoms = [atom(8, 0.0), atom(1, 0.96), atom(1, -0.96)];
        assert_eq!(bonds(&atoms, 1.15, false), vec![(0, 1), (0, 2)]);
        assert!(bonds(&atoms, 0.5, false).is_empty());
        // Ti–Ti a 2.7 Å: solo si se piden enlaces metal–metal.
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
