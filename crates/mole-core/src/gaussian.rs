use crate::{Atom, EnergySource, Error, ErrorKind, Structure};
use std::path::Path;

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
            _ if warnings.len() >= 20 => {}
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

fn push_step(
    lines: &[&str],
    start: usize,
    energy: Option<f64>,
    path: &Path,
    steps: &mut Vec<Structure>,
    warnings: &mut Vec<String>,
) -> Result<(), Error> {
    let atoms = parse_orientation(lines, start, path, warnings)?;
    crate::add_step(steps, atoms, energy, EnergySource::GaussianScf, path, start)
}

pub fn parse(text: &str, path: &Path) -> Result<(Vec<Structure>, Vec<String>), Error> {
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
    let mut steps = Vec::new();
    let (mut last_std, mut last_inp, mut last_used) = (None, None, None);
    let mut normal_end = false;
    for (i, line) in lines.iter().enumerate() {
        if line.contains("Standard orientation:") {
            last_std = Some(i);
        } else if line.contains("Input orientation:") {
            last_inp = Some(i);
        } else if line.contains("SCF Done:") {
            let energy = line
                .split_once('=')
                .and_then(|(_, rest)| rest.split_whitespace().next())
                .and_then(|t| t.parse::<f64>().ok())
                .filter(|v| v.is_finite());
            if energy.is_none() && warnings.len() < 20 {
                warnings.push(format!(
                    "{}:{}: 'SCF Done' line without a readable energy",
                    path.display(),
                    i + 1
                ));
            }
            if let Some(start) = last_std.or(last_inp) {
                if let Err(e) = push_step(&lines, start, energy, path, &mut steps, &mut warnings)
                    && warnings.len() < 20
                {
                    warnings.push(format!("geometry skipped: {e}"));
                }
                last_used = Some(start);
            }
            (last_std, last_inp) = (None, None);
        } else if line.contains("Normal termination of Gaussian") {
            normal_end = true;
        }
    }
    if steps.is_empty() {
        let start = last_std
            .or(last_inp)
            .or(last_used)
            .ok_or_else(|| Error::parse(path, lines.len().max(1), "no geometry found"))?;
        warnings.clear();
        push_step(&lines, start, None, path, &mut steps, &mut warnings)?;
        warnings.push(format!("{}: no 'SCF Done' energy", path.display()));
    }
    crate::name_steps(&mut steps, path);
    if !normal_end
        || lines
            .iter()
            .rev()
            .take(20)
            .any(|l| l.contains("Error termination"))
    {
        warnings.push(format!(
            "{}: calculation did not terminate normally",
            path.display()
        ));
    }
    Ok((steps, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn p(t: &str) -> Result<(Vec<Structure>, Vec<String>), Error> {
        parse(t, Path::new("dir/calc_stage1.out"))
    }

    #[test]
    fn every_optimization_step() {
        let (s, w) = p(SAMPLE).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].energy, Some(-76.4));
        assert_eq!(s[0].atoms[0].pos, [0.0, 0.0, 0.1]);
        assert_eq!(s[1].energy, Some(-76.42));
        assert_eq!(s[1].energy_source, Some(EnergySource::GaussianScf));
        assert_eq!(s[1].atoms.len(), 3);
        assert_eq!(s[1].atoms[0].pos, [0.0, 0.0, 0.11]);
        assert_eq!(
            (s[0].label.as_str(), s[1].label.as_str()),
            ("calc_stage1 1", "calc_stage1 2")
        );
        assert_eq!(s[1].line, 23);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("Z = 0"));
        let tail = SAMPLE.split(" Normal termination").next().unwrap();
        let last = &tail[tail.rfind("Standard orientation:").unwrap()..];
        let (s, _) = p(&format!("{tail}{last} Normal termination\n")).unwrap();
        assert_eq!(
            s.len(),
            2,
            "un cálculo repetido en la misma geometría no es un paso nuevo"
        );
    }

    #[test]
    fn incomplete_or_foreign_outputs() {
        let cut = SAMPLE.split(" SCF Done").next().unwrap();
        let (s, w) = p(cut).unwrap();
        assert_eq!(
            (s.len(), s[0].energy, s[0].label.as_str()),
            (1, None, "calc_stage1")
        );
        assert!(w.iter().any(|w| w.contains("no 'SCF Done'")));
        assert!(w.iter().any(|w| w.contains("did not terminate normally")));
        let trunc: String = SAMPLE.lines().take(9).collect::<Vec<_>>().join("\n");
        assert!(p(&trunc).unwrap_err().msg.contains("unterminated"));
        assert!(
            p(" Entering Gaussian System\n")
                .unwrap_err()
                .msg
                .contains("geometry")
        );
        assert_eq!(
            p("* O   R   C   A *\n").unwrap_err().kind,
            ErrorKind::Unsupported
        );
        for bad in [
            SAMPLE.replace("0.760000   -0.500000", "0.76abc   -0.500000"),
            SAMPLE.replace(
                "-0.770000   -0.590000\n      4",
                "-0.77xx   -0.590000\n      4",
            ),
        ] {
            let (s, w) = p(&bad).unwrap();
            assert_eq!(s.len(), 1, "la geometría corrupta se omite");
            assert!(w.iter().any(|w| w.contains("row")), "{w:?}");
        }
    }

    #[test]
    fn mutations_never_panic() {
        crate::mutations(SAMPLE, 5_000, |t| {
            let _ = p(t);
        });
    }
}
