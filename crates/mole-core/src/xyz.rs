//! XYZ multi-frame: `n`, comentario, `n` líneas `símbolo x y z` (Å), repetido.

use crate::{Atom, EnergySource, Error, Structure, elements};
use std::path::Path;

/// Máximo de átomos por estructura.
pub const MAX_ATOMS: usize = 1_000_000;

/// Energía y etiqueta de la línea de comentario. Convenciones reconocidas, en este orden:
/// 1. clave `E=<número>` o `energy=<número>` en cualquier posición (`label=3a  E=-5131.15 Eh …`),
///    o `E <número>` separado (trayectorias de ORCA: `Coordinates from ORCA-job h2o E -74.96`);
/// 2. GLOMOS: el primer campo es un número (`-3219847.92  stage100001`), el segundo la etiqueta.
///
/// La etiqueta sale de `label=<texto>` o, en GLOMOS, del segundo campo.
pub fn energy_from_comment(comment: &str) -> (Option<f64>, Option<String>) {
    let num = |s: &str| s.parse::<f64>().ok().filter(|v| v.is_finite());
    let mut energy = None;
    let mut label = None;
    let mut prev = "";
    for tok in comment.split_whitespace() {
        let after_e = std::mem::replace(&mut prev, tok) == "E";
        if energy.is_none() && after_e {
            energy = num(tok);
        }
        let Some((key, value)) = tok.split_once('=') else {
            continue;
        };
        if energy.is_none() && (key == "E" || key.eq_ignore_ascii_case("energy")) {
            energy = num(value);
        } else if label.is_none() && key == "label" && !value.is_empty() {
            label = Some(value.to_string());
        }
    }
    if energy.is_none() {
        let mut it = comment.split_whitespace();
        energy = it.next().and_then(num);
        if energy.is_some() && label.is_none() {
            label = it.next().map(str::to_string);
        }
    }
    (energy, label)
}

/// Lee todas las estructuras de un XYZ. Nunca entra en pánico; ante cualquier dato inválido
/// devuelve el error con su número de línea.
pub fn parse(text: &str, path: &Path) -> Result<Vec<Structure>, Error> {
    let lines: Vec<&str> = text.lines().collect();
    let mut frames = Vec::new();
    let mut i = 0;
    loop {
        // Líneas en blanco entre estructuras o al final.
        while i < lines.len() && lines[i].trim().is_empty() {
            i += 1;
        }
        if i == lines.len() {
            break;
        }
        let start = i + 1;
        let count_tok = lines[i].split_whitespace().next().unwrap_or("");
        let n: usize = count_tok.parse().map_err(|_| {
            Error::parse(
                path,
                start,
                format!("expected the number of atoms, found '{count_tok}'"),
            )
        })?;
        if n == 0 || n > MAX_ATOMS {
            return Err(Error::parse(
                path,
                start,
                format!("number of atoms out of range: {n} (1..={MAX_ATOMS})"),
            ));
        }
        // Validar el contador contra las líneas reales antes de reservar memoria.
        if lines.len() - i - 1 < n + 1 {
            return Err(Error::parse(
                path,
                start,
                format!(
                    "{n} atoms announced, but the file ends at line {}",
                    lines.len()
                ),
            ));
        }
        let comment = lines[i + 1].trim().to_string();
        let mut atoms = Vec::with_capacity(n);
        for (k, line) in lines[i + 2..i + 2 + n].iter().enumerate() {
            let lineno = i + 3 + k;
            let toks: Vec<&str> = line.split_whitespace().take(4).collect();
            if toks.len() < 4 {
                return Err(Error::parse(path, lineno, "expected 'symbol x y z'"));
            }
            let z = elements::parse_element(toks[0]).ok_or_else(|| {
                Error::parse(path, lineno, format!("unknown element '{}'", toks[0]))
            })?;
            let mut pos = [0.0; 3];
            for (c, tok) in pos.iter_mut().zip(&toks[1..]) {
                *c = tok
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite())
                    .ok_or_else(|| {
                        Error::parse(path, lineno, format!("invalid coordinate '{tok}'"))
                    })?;
            }
            atoms.push(Atom { z, pos });
        }
        let (energy, label) = energy_from_comment(&comment);
        frames.push(Structure {
            atoms,
            energy,
            energy_source: energy.map(|_| EnergySource::XyzComment),
            label: label.unwrap_or_default(),
            comment,
            path: path.to_path_buf(),
            line: start,
        });
        i += n + 2;
    }
    if frames.is_empty() {
        return Err(Error::parse(path, 1, "the file contains no structures"));
    }
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(text: &str) -> Result<Vec<Structure>, Error> {
        parse(text, Path::new("t.xyz"))
    }

    #[test]
    fn comment_conventions() {
        assert_eq!(
            energy_from_comment("-3219847.92366207     stage100001"),
            (Some(-3219847.92366207), Some("stage100001".into()))
        );
        assert_eq!(
            energy_from_comment(
                "label=3a  E=-5131.15495902 Eh  dE=0.00  dE0=0.00  dG=0.99 kcal/mol  P_G=8.6%"
            ),
            (Some(-5131.15495902), Some("3a".into()))
        );
        assert_eq!(
            energy_from_comment("Al8Ti4O20 isomero 4a  E=-6841.64934323 Eh  PBE0/def2-TZVP"),
            (Some(-6841.64934323), None)
        );
        assert_eq!(
            energy_from_comment("energy=-1.5 pbc=\"F F F\""),
            (Some(-1.5), None)
        );
        assert_eq!(
            energy_from_comment("Coordinates from ORCA-job h2o E -74.962421895501"),
            (Some(-74.962421895501), None)
        );
        assert_eq!(energy_from_comment("agua optimizada"), (None, None));
        assert_eq!(energy_from_comment("E x"), (None, None));
        assert_eq!(energy_from_comment(""), (None, None));
        assert_eq!(energy_from_comment("nan x"), (None, None));
        assert_eq!(energy_from_comment("E=abc"), (None, None));
    }

    #[test]
    fn multi_frame_with_blank_lines() {
        let text = "3  \n-10.5 s1\nO 0 0 0\nH 0.96 0 0\nH -0.24 0.93 0\n\n3\nsin energía\nO 0 0 0\nH 1 0 0\nH 0 1 0 extra col\n\n";
        let f = p(text).unwrap();
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].energy, Some(-10.5));
        assert_eq!(f[0].label, "s1");
        assert_eq!(f[0].line, 1);
        assert_eq!(f[1].energy, None);
        assert_eq!(f[1].energy_source, None);
        assert_eq!(f[1].line, 7);
        assert_eq!(f[1].atoms[2].pos, [0.0, 1.0, 0.0]);
    }

    #[test]
    fn invalid_inputs_report_line() {
        let err = |t| p(t).unwrap_err();
        assert!(err("").msg.contains("no structures"));
        assert!(err("x\n").msg.contains("number of atoms"));
        assert_eq!(err("0\nc\n").line, Some(1));
        // Contador enorme: debe fallar sin reservar memoria.
        assert!(err("999999\nc\nO 0 0 0\n").msg.contains("ends at line"));
        assert!(
            err("99999999999999999999\nc\n")
                .msg
                .contains("number of atoms")
        );
        let e = err("2\nc\nO 0 0 0\nH 1 0\n");
        assert_eq!(e.line, Some(4));
        let e = err("1\nc\nQq 0 0 0\n");
        assert!(e.msg.contains("unknown element"));
        let e = err("1\nc\nO 0 inf 0\n");
        assert!(e.msg.contains("invalid coordinate"));
        let e = err("1\nc\nO 0 0 1,5\n");
        assert_eq!(e.line, Some(3));
    }

    /// Mutaciones pseudoaleatorias de un XYZ válido: el lector nunca debe entrar en pánico.
    // ponytail: sustituto de cargo-fuzz (requiere nightly); añadir un fuzz target si se adopta nightly.
    #[test]
    fn mutations_never_panic() {
        let base = "2\n-1.0 a\nO 0.0 0.0 0.0\nH 0.96 0.0 0.0\n2\nE=-2\nO 0 0 0\nH 1 0 0\n";
        crate::mutations(base, 20_000, |t| {
            let _ = p(t);
        });
    }
}
