//! Salidas de Gaussian (`.out`/`.log`): geometría final y última energía SCF.
//!
//! Probado con Gaussian 16 Rev. B.01 (salidas de GLOMOS). Otras versiones no se declaran soportadas.

use crate::{Atom, EnergySource, Error, ErrorKind, Structure};
use std::path::Path;

/// Tabla de coordenadas que sigue a `Standard orientation:` o `Input orientation:`:
/// 4 líneas de cabecera y filas `centro Z tipo x y z` (o `centro Z x y z`) hasta una línea de guiones.
fn parse_orientation(
    lines: &[&str],
    start: usize,
    path: &Path,
    warnings: &mut Vec<String>,
) -> Result<Vec<Atom>, Error> {
    let mut atoms = Vec::new();
    for (k, line) in lines.iter().enumerate().skip(start + 5) {
        if line.trim_start().starts_with("---") {
            return Ok(atoms);
        }
        let toks: Vec<&str> = line.split_whitespace().collect();
        let lineno = k + 1;
        let bad = || Error::parse(path, lineno, "invalid coordinate row");
        if !(5..=6).contains(&toks.len()) {
            return Err(bad());
        }
        let z: i32 = toks[1].parse().map_err(|_| bad())?;
        let mut pos = [0.0; 3];
        for (c, tok) in pos.iter_mut().zip(&toks[toks.len() - 3..]) {
            *c = tok
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or_else(bad)?;
        }
        match u8::try_from(z) {
            Ok(z @ 1..=118) => atoms.push(Atom { z, pos }),
            // Átomos fantasma (Bq, Z = 0) o ficticios (X, Z = -1): no se dibujan, pero se avisa.
            _ => warnings.push(format!(
                "{}:{lineno}: center with Z = {z} skipped (ghost or dummy atom)",
                path.display()
            )),
        }
    }
    Err(Error::parse(
        path,
        start + 1,
        "unterminated coordinate table",
    ))
}

/// Lee la última geometría (`Standard orientation`, o `Input orientation` si no hay otra) y la
/// última energía `SCF Done`. Devuelve también avisos (sin energía, terminación anormal…).
pub fn parse(text: &str, path: &Path) -> Result<(Structure, Vec<String>), Error> {
    let lines: Vec<&str> = text.lines().collect();
    if !lines
        .iter()
        .take(50)
        .any(|l| l.contains("Gaussian") || l.contains("Entering Link"))
    {
        return Err(Error {
            kind: ErrorKind::Unsupported,
            path: path.to_path_buf(),
            line: None,
            msg: "does not look like a Gaussian output".into(),
        });
    }
    let mut warnings = Vec::new();
    let (mut last_std, mut last_inp, mut energy) = (None, None, None);
    let mut normal_end = false;
    for (i, line) in lines.iter().enumerate() {
        if line.contains("Standard orientation:") {
            last_std = Some(i);
        } else if line.contains("Input orientation:") {
            last_inp = Some(i);
        } else if line.contains("SCF Done:") {
            // " SCF Done:  E(RPBE1PBE) =  -5131.15495934     A.U. after   19 cycles"
            energy = line
                .split_once('=')
                .and_then(|(_, rest)| rest.split_whitespace().next())
                .and_then(|t| t.parse::<f64>().ok())
                .filter(|v| v.is_finite());
            if energy.is_none() {
                warnings.push(format!(
                    "{}:{}: 'SCF Done' line without a readable energy",
                    path.display(),
                    i + 1
                ));
            }
        } else if line.contains("Normal termination of Gaussian") {
            normal_end = true;
        }
    }
    let start = last_std
        .or(last_inp)
        .ok_or_else(|| Error::parse(path, lines.len().max(1), "no geometry found"))?;
    let atoms = parse_orientation(&lines, start, path, &mut warnings)?;
    if atoms.is_empty() {
        return Err(Error::parse(path, start + 1, "geometry without atoms"));
    }
    if energy.is_none() {
        warnings.push(format!("{}: no 'SCF Done' energy", path.display()));
    }
    if !normal_end
        || lines
            .iter()
            .rev()
            .take(20)
            .any(|l| l.contains("Error termination"))
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
        energy_source: energy.map(|_| EnergySource::GaussianScf),
        comment: format!("Gaussian: {}", path.display()),
        label,
        path: path.to_path_buf(),
        line: start + 1,
    };
    Ok((structure, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Extracto sintético con el formato de Gaussian 16 (dos geometrías, dos energías).
    const SAMPLE: &str = " Entering Gaussian System, Link 0=g16
 Gaussian 16:  EM64L-G16RevB.01 20-Dec-2017
                         Standard orientation:
 ---------------------------------------------------------------------
 Center     Atomic      Atomic             Coordinates (Angstroms)
 Number     Number       Type             X           Y           Z
 ---------------------------------------------------------------------
      1          8           0        0.000000    0.000000    0.100000
      2          1           0        0.000000    0.760000   -0.500000
      3          1           0        0.000000   -0.760000   -0.500000
 ---------------------------------------------------------------------
 SCF Done:  E(RB3LYP) =  -76.4000000000     A.U. after   10 cycles
                          Input orientation:
 ---------------------------------------------------------------------
 Center     Atomic      Atomic             Coordinates (Angstroms)
 Number     Number       Type             X           Y           Z
 ---------------------------------------------------------------------
      1          8           0        1.000000    0.000000    0.000000
      2          1           0        1.000000    0.770000   -0.590000
      3          1           0        1.000000   -0.770000   -0.590000
      4          0           0        9.000000    9.000000    9.000000
 ---------------------------------------------------------------------
                         Standard orientation:
 ---------------------------------------------------------------------
 Center     Atomic      Atomic             Coordinates (Angstroms)
 Number     Number       Type             X           Y           Z
 ---------------------------------------------------------------------
      1          8           0        0.000000    0.000000    0.110000
      2          1           0        0.000000    0.770000   -0.590000
      3          1           0        0.000000   -0.770000   -0.590000
      4          0           0        9.000000    9.000000    9.000000
 ---------------------------------------------------------------------
 SCF Done:  E(RB3LYP) =  -76.4200000000     A.U. after    6 cycles
 Normal termination of Gaussian 16 at Sun Feb 12 00:02:13 2023.
";

    fn p(t: &str) -> Result<(Structure, Vec<String>), Error> {
        parse(t, Path::new("dir/calc_stage1.out"))
    }

    #[test]
    fn last_standard_orientation_and_energy() {
        let (s, w) = p(SAMPLE).unwrap();
        assert_eq!(s.energy, Some(-76.42));
        assert_eq!(s.energy_source, Some(EnergySource::GaussianScf));
        assert_eq!(s.atoms.len(), 3);
        assert_eq!(s.atoms[0].pos, [0.0, 0.0, 0.11]);
        assert_eq!(s.label, "calc_stage1");
        assert_eq!(s.line, 23);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("Z = 0"));
    }

    #[test]
    fn incomplete_or_foreign_outputs() {
        // Sin «Normal termination» ni energía: se carga con avisos.
        let cut = SAMPLE.split(" SCF Done").next().unwrap();
        let (s, w) = p(cut).unwrap();
        assert_eq!(s.energy, None);
        assert!(w.iter().any(|w| w.contains("no 'SCF Done'")));
        assert!(w.iter().any(|w| w.contains("did not terminate normally")));
        // Tabla truncada.
        let trunc: String = SAMPLE.lines().take(9).collect::<Vec<_>>().join("\n");
        assert!(p(&trunc).unwrap_err().msg.contains("unterminated"));
        // Sin geometría.
        assert!(
            p(" Entering Gaussian System\n")
                .unwrap_err()
                .msg
                .contains("geometry")
        );
        // Otro programa.
        assert_eq!(
            p("* O   R   C   A *\n").unwrap_err().kind,
            ErrorKind::Unsupported
        );
        // Fila corrupta.
        let bad = SAMPLE.replace("0.760000   -0.500000", "0.76abc   -0.500000");
        assert!(
            p(&bad).is_ok(),
            "la fila corrupta está en una geometría anterior"
        );
        let bad = SAMPLE.replace(
            "-0.770000   -0.590000\n      4",
            "-0.77xx   -0.590000\n      4",
        );
        assert!(p(&bad).unwrap_err().msg.contains("row"));
    }

    #[test]
    fn mutations_never_panic() {
        crate::mutations(SAMPLE, 5_000, |t| {
            let _ = p(t);
        });
    }
}
