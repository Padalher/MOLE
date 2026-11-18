use crate::{Atom, EnergySource, Error, Structure, elements};
use std::path::Path;

const GEOMETRY: &str = "CARTESIAN COORDINATES (ANGSTROEM)";
const ENERGY: &str = "FINAL SINGLE POINT ENERGY";

pub fn is_orca(text: &str) -> bool {
    text.lines().take(100).any(|l| l.contains("O   R   C   A"))
}

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
            None if warnings.len() >= 20 => {}
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

pub fn parse(text: &str, path: &Path) -> Result<(Vec<Structure>, Vec<String>), Error> {
    let lines: Vec<&str> = text.lines().collect();
    let mut warnings = Vec::new();
    let mut steps = Vec::new();
    let (mut pending, mut last_geom) = (None, None);
    for (i, line) in lines.iter().enumerate() {
        if line.contains(GEOMETRY) {
            (pending, last_geom) = (Some(i), Some(i));
        } else if let Some(rest) = line.trim_start().strip_prefix(ENERGY) {
            let energy = rest.trim().parse::<f64>().ok().filter(|v| v.is_finite());
            if energy.is_none() && warnings.len() < 20 {
                warnings.push(format!(
                    "{}:{}: '{ENERGY}' line without a readable energy",
                    path.display(),
                    i + 1
                ));
            }
            if let Some(start) = pending.take()
                && let Err(e) = parse_geometry(&lines, start, path, &mut warnings).and_then(|a| {
                    crate::add_step(&mut steps, a, energy, EnergySource::OrcaFinal, path, start)
                })
                && warnings.len() < 20
            {
                warnings.push(format!("geometry skipped: {e}"));
            }
        }
    }
    if steps.is_empty() {
        let start =
            last_geom.ok_or_else(|| Error::parse(path, lines.len().max(1), "no geometry found"))?;
        warnings.clear();
        let atoms = parse_geometry(&lines, start, path, &mut warnings)?;
        crate::add_step(
            &mut steps,
            atoms,
            None,
            EnergySource::OrcaFinal,
            path,
            start,
        )?;
        warnings.push(format!("{}: no '{ENERGY}'", path.display()));
    }
    crate::name_steps(&mut steps, path);
    if !lines
        .iter()
        .rev()
        .take(20)
        .any(|l| l.contains("ORCA TERMINATED NORMALLY"))
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

    fn p(t: &str) -> Result<(Vec<Structure>, Vec<String>), Error> {
        parse(t, Path::new("dir/h2o.out"))
    }

    #[test]
    fn every_geometry_with_its_energy() {
        assert!(is_orca(SAMPLE));
        let (s, w) = p(SAMPLE).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].energy, Some(-74.962421895501));
        assert_eq!(s[1].energy, Some(-74.965901191476));
        assert_eq!(s[1].energy_source, Some(EnergySource::OrcaFinal));
        assert_eq!(s[1].atoms.len(), 3);
        assert_eq!(s[1].atoms[1].pos, [0.0, 0.758105, -0.471927]);
        assert_eq!(
            (s[0].label.as_str(), s[1].label.as_str()),
            ("h2o 1", "h2o 2")
        );
        assert_eq!(s[1].line, 19);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("'DA'"));
    }

    #[test]
    fn incomplete_outputs() {
        let cut = SAMPLE.rsplit_once("FINAL SINGLE").unwrap().0;
        let (s, w) = p(cut).unwrap();
        assert_eq!((s.len(), s[0].energy), (1, Some(-74.962421895501)));
        assert!(w.iter().any(|w| w.contains("did not terminate normally")));
        let first = SAMPLE.split("FINAL SINGLE").next().unwrap();
        let (s, w) = p(first).unwrap();
        assert_eq!(
            (s.len(), s[0].energy, s[0].label.as_str()),
            (1, None, "h2o")
        );
        assert!(w.iter().any(|w| w.contains("no 'FINAL")));
        let trunc: String = SAMPLE.lines().take(9).collect::<Vec<_>>().join("\n");
        assert!(p(&trunc).unwrap_err().msg.contains("unterminated"));
        assert!(
            p("* O   R   C   A *\n")
                .unwrap_err()
                .msg
                .contains("geometry")
        );
        let bad = SAMPLE.replace("0.758105   -0.471927", "0.75x   -0.471927");
        let (s, w) = p(&bad).unwrap();
        assert_eq!(s.len(), 1, "la geometría corrupta se omite");
        assert!(w.iter().any(|w| w.contains("row")), "{w:?}");
    }

    #[test]
    fn mutations_never_panic() {
        crate::mutations(SAMPLE, 5_000, |t| {
            let _ = p(t);
        });
    }
}
