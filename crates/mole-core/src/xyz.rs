use crate::{Atom, Column, EnergySource, Error, Structure, elements};
use std::path::Path;

pub const MAX_ATOMS: usize = 1_000_000;

const MAX_COLUMNS: usize = 64;

fn properties(comment: &str) -> Vec<(&str, usize, usize)> {
    let Some(spec) = comment
        .split_whitespace()
        .find_map(|t| t.strip_prefix("Properties="))
    else {
        return Vec::new();
    };
    let f: Vec<&str> = spec.split(':').collect();
    if !f.len().is_multiple_of(3) || !f.starts_with(&["species", "S", "1", "pos", "R", "3"]) {
        return Vec::new();
    }
    let (mut col, mut out) = (4, Vec::new());
    for t in f[6..].chunks(3) {
        match t[2].parse::<usize>() {
            Ok(n) if n > 0 && col + n <= MAX_COLUMNS => {
                if matches!(t[1], "R" | "I") {
                    out.push((t[0], col, n));
                }
                col += n;
            }
            _ => return Vec::new(),
        }
    }
    out
}

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

pub fn parse(text: &str, path: &Path) -> Result<Vec<Structure>, Error> {
    let lines: Vec<&str> = text.lines().collect();
    let mut frames = Vec::new();
    let mut i = 0;
    loop {
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
        let props = properties(&comment);
        let needed = props.iter().map(|&(_, c, w)| c + w).max().unwrap_or(4);
        let mut columns: Vec<Column> = props
            .iter()
            .map(|&(name, _, width)| Column {
                name: name.to_string(),
                width,
                values: Vec::new(),
            })
            .collect();
        let mut atoms = Vec::with_capacity(n);
        for (k, line) in lines[i + 2..i + 2 + n].iter().enumerate() {
            let lineno = i + 3 + k;
            let toks: Vec<&str> = line.split_whitespace().take(needed).collect();
            if toks.len() < needed {
                columns.clear();
            }
            for (col, &(_, first, width)) in columns.iter_mut().zip(&props) {
                col.values
                    .extend(toks[first..first + width].iter().map(|t| {
                        t.parse::<f64>()
                            .ok()
                            .filter(|v| v.is_finite())
                            .unwrap_or(f64::NAN)
                    }));
            }
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
            columns,
            grid: None,
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

    #[test]
    fn extended_columns() {
        let text = "2\nE=-1 Properties=species:S:1:pos:R:3:tag:S:1:q:R:1:forces:R:3\n\
                    H 0 0 0 a 0.5 1 2 3\nH 0 0 0.7 b nan -1 -2 -3e-5\n\
                    1\nProperties=species:S:1:pos:R:3:q:R:1\nO 0 0 0\n\
                    1\nProperties=pos:R:3:species:S:1:q:R:1\nO 0 0 0 1\n";
        let f = p(text).unwrap();
        let q = f[0].column("q").unwrap();
        assert_eq!((q.width, q.values[0]), (1, 0.5));
        assert!(q.values[1].is_nan());
        let forces = f[0].column("forces").unwrap();
        assert_eq!(forces.values[3..], [-1.0, -2.0, -3e-5]);
        assert!(
            f[0].column("tag").is_none(),
            "las columnas de texto no se guardan"
        );
        assert!(f[1].columns.is_empty(), "línea sin la columna anunciada");
        assert!(f[2].columns.is_empty(), "no empieza con species y pos");
        assert!(properties("Properties=species:S:1:pos:R:3:x:R:99").is_empty());
        assert!(properties("Properties=species:S:1:pos:R:3:x:R").is_empty());
    }

    #[test]
    fn mutations_never_panic() {
        let base = "2\n-1.0 a\nO 0.0 0.0 0.0\nH 0.96 0.0 0.0\n2\nE=-2 Properties=species:S:1:pos:R:3:f:R:3\nO 0 0 0 1 2 3\nH 1 0 0 4 5 6\n";
        crate::mutations(base, 20_000, |t| {
            let _ = p(t);
        });
    }
}
