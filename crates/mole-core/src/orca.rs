//! Salidas de ORCA (`.out`/`.log`): última geometría y su `FINAL SINGLE POINT ENERGY`.
//!
//! Probado con ORCA 6.1.0. Otras versiones no se declaran soportadas.

use crate::{Atom, EnergySource, Error, Structure, elements};
use std::path::Path;

const GEOMETRY: &str = "CARTESIAN COORDINATES (ANGSTROEM)";
const ENERGY: &str = "FINAL SINGLE POINT ENERGY";

/// ¿Es una salida de ORCA? (cabecera `* O   R   C   A *` al principio).
pub fn is_orca(text: &str) -> bool {
    text.lines().take(100).any(|l| l.contains("O   R   C   A"))
}

/// Tabla tras `CARTESIAN COORDINATES (ANGSTROEM)`: una línea de guiones y filas `símbolo x y z`
/// hasta una línea en blanco.
fn parse_geometry(
    lines: &[&str],
    start: usize,
    path: &Path,
    warnings: &mut Vec<String>,
) -> Result<Vec<Atom>, Error> {
    let mut atoms = Vec::new();
    for (k, line) in lines.iter().enumerate().skip(start + 2) {
        let toks: Vec<&str> = line.split_whitespace().collect();
        if toks.is_empty() {
            return Ok(atoms);
        }
        let lineno = k + 1;
        let bad = || Error::parse(path, lineno, "invalid coordinate row");
        if toks.len() != 4 {
            return Err(bad());
        }
        let mut pos = [0.0; 3];
        for (c, tok) in pos.iter_mut().zip(&toks[1..]) {
            *c = tok
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or_else(bad)?;
        }
        match elements::parse_element(toks[0]) {
            Some(z) => atoms.push(Atom { z, pos }),
            // Átomos ficticios (DA) o cargas puntuales (Q): no se dibujan, pero se avisa.
            None => warnings.push(format!(
                "{}:{lineno}: center '{}' skipped (not an element)",
                path.display(),
                toks[0]
            )),
        }
    }
    Err(Error::parse(
        path,
        start + 1,
        "unterminated coordinate table",
    ))
}

/// Lee la última geometría y la última `FINAL SINGLE POINT ENERGY` que la sigue (la energía de una
/// geometría anterior no se asigna a la última). Devuelve también avisos.
pub fn parse(text: &str, path: &Path) -> Result<(Structure, Vec<String>), Error> {
    let lines: Vec<&str> = text.lines().collect();
    let mut warnings = Vec::new();
    let (mut last_geom, mut energy) = (None, None);
    for (i, line) in lines.iter().enumerate() {
        if line.contains(GEOMETRY) {
            last_geom = Some(i);
            energy = None;
        } else if let Some(rest) = line.trim_start().strip_prefix(ENERGY) {
            // "FINAL SINGLE POINT ENERGY     -1710.170750050696"
            energy = rest.trim().parse::<f64>().ok().filter(|v| v.is_finite());
            if energy.is_none() {
                warnings.push(format!(
                    "{}:{}: '{ENERGY}' line without a readable energy",
                    path.display(),
                    i + 1
                ));
            }
        }
    }
    let start =
        last_geom.ok_or_else(|| Error::parse(path, lines.len().max(1), "no geometry found"))?;
    let atoms = parse_geometry(&lines, start, path, &mut warnings)?;
    if atoms.is_empty() {
        return Err(Error::parse(path, start + 1, "geometry without atoms"));
    }
    if energy.is_none() {
        warnings.push(format!(
            "{}: the last geometry has no '{ENERGY}'",
            path.display()
        ));
    }
    if !lines
        .iter()
        .rev()
        .take(20)
        .any(|l| l.contains("ORCA TERMINATED NORMALLY"))
    {
        warnings.push(format!(
            "{}: calculation did not terminate normally; showing the last geometry",
            path.display()
        ));
    }
    let label = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();
    let structure = Structure {
        atoms,
        energy,
        energy_source: energy.map(|_| EnergySource::OrcaFinal),
        comment: format!("ORCA: {}", path.display()),
        label,
        path: path.to_path_buf(),
        line: start + 1,
    };
    Ok((structure, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Extracto con el formato de ORCA 6.1 (optimización de H2O generada para este proyecto).
    const SAMPLE: &str = "
                                 *****************
                                 * O   R   C   A *
                                 *****************
---------------------------------
CARTESIAN COORDINATES (ANGSTROEM)
---------------------------------
  O      0.000000    0.000000    0.120000
  H      0.000000    0.800000   -0.450000
  H      0.000000   -0.800000   -0.450000

----------------------------
CARTESIAN COORDINATES (A.U.)
----------------------------
-------------------------   --------------------
FINAL SINGLE POINT ENERGY       -74.962421895501
-------------------------   --------------------
---------------------------------
CARTESIAN COORDINATES (ANGSTROEM)
---------------------------------
  O      0.000000   -0.000000    0.163853
  H      0.000000    0.758105   -0.471927
  H      0.000000   -0.758105   -0.471927
  DA     1.000000    1.000000    1.000000

-------------------------   --------------------
FINAL SINGLE POINT ENERGY       -74.965901191476
-------------------------   --------------------
                             ****ORCA TERMINATED NORMALLY****
";

    fn p(t: &str) -> Result<(Structure, Vec<String>), Error> {
        parse(t, Path::new("dir/h2o.out"))
    }

    #[test]
    fn last_geometry_and_its_energy() {
        assert!(is_orca(SAMPLE));
        let (s, w) = p(SAMPLE).unwrap();
        assert_eq!(s.energy, Some(-74.965901191476));
        assert_eq!(s.energy_source, Some(EnergySource::OrcaFinal));
        assert_eq!(s.atoms.len(), 3);
        assert_eq!(s.atoms[1].pos, [0.0, 0.758105, -0.471927]);
        assert_eq!(s.label, "h2o");
        assert_eq!(s.line, 19);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("'DA'"));
    }

    #[test]
    fn incomplete_outputs() {
        // Cortado tras la última geometría: no se le asigna la energía de la anterior.
        let cut = SAMPLE.rsplit_once("FINAL SINGLE").unwrap().0;
        let (s, w) = p(cut).unwrap();
        assert_eq!(s.energy, None);
        assert!(w.iter().any(|w| w.contains("has no")));
        assert!(w.iter().any(|w| w.contains("did not terminate normally")));
        // Tabla truncada.
        let trunc: String = SAMPLE.lines().take(9).collect::<Vec<_>>().join("\n");
        assert!(p(&trunc).unwrap_err().msg.contains("unterminated"));
        // Sin geometría.
        assert!(
            p("* O   R   C   A *\n")
                .unwrap_err()
                .msg
                .contains("geometry")
        );
        // Fila corrupta en la última geometría.
        let bad = SAMPLE.replace("0.758105   -0.471927", "0.75x   -0.471927");
        assert!(p(&bad).unwrap_err().msg.contains("row"));
    }

    #[test]
    fn mutations_never_panic() {
        crate::mutations(SAMPLE, 5_000, |t| {
            let _ = p(t);
        });
    }
}
