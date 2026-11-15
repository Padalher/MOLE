use crate::{Atom, Error, Structure, xyz::MAX_ATOMS};
use std::path::Path;

const BOHR_TO_ANGSTROM: f64 = 0.529_177_210_903;

#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    pub n: [usize; 3],
    pub origin: [f64; 3],
    pub axes: [[f64; 3]; 3],
    pub values: Vec<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Face {
    pub corners: [[f64; 3]; 4],
    pub normal: [f64; 3],
}

pub fn parse(text: &str, path: &Path) -> Result<(Structure, Vec<String>), Error> {
    let err = |line: usize, msg: String| Error::parse(path, line, msg);
    let lines: Vec<&str> = text.lines().collect();
    let fields = |i: usize| -> Result<Vec<&str>, Error> {
        let line = lines
            .get(i)
            .ok_or_else(|| err(i + 1, "unexpected end of file".into()))?;
        Ok(line.split_ascii_whitespace().collect())
    };
    let real = |tok: Option<&&str>, i: usize| -> Result<f64, Error> {
        tok.and_then(|t| t.parse::<f64>().ok())
            .filter(|v| v.is_finite())
            .ok_or_else(|| err(i + 1, "expected a number".into()))
    };
    let count = |tok: Option<&&str>, i: usize| -> Result<i64, Error> {
        tok.and_then(|t| t.parse::<i64>().ok())
            .ok_or_else(|| err(i + 1, "expected an integer".into()))
    };

    let head = fields(2)?;
    let natoms = count(head.first(), 2)?;
    let atom_lines = usize::try_from(natoms.unsigned_abs()).unwrap_or(usize::MAX);
    if atom_lines == 0 || atom_lines > MAX_ATOMS {
        return Err(err(3, format!("invalid atom count {natoms}")));
    }
    let origin = [1, 2, 3].map(|k| real(head.get(k), 2));
    let mut sets = match head.get(4) {
        Some(_) => usize::try_from(count(head.get(4), 2)?).unwrap_or(0),
        None => 1,
    };

    let mut n = [0usize; 3];
    let mut axes = [[0.0; 3]; 3];
    let mut unit = BOHR_TO_ANGSTROM;
    for a in 0..3 {
        let i = 3 + a;
        let f = fields(i)?;
        let points = count(f.first(), i)?;
        if a == 0 && points < 0 {
            unit = 1.0;
        }
        n[a] = usize::try_from(points.unsigned_abs()).unwrap_or(0);
        if n[a] < 2 {
            return Err(err(
                i + 1,
                format!("invalid number of grid points {points}"),
            ));
        }
        for (k, v) in axes[a].iter_mut().enumerate() {
            *v = real(f.get(k + 1), i)? * unit;
        }
    }
    let mut pos = [0.0; 3];
    for (p, o) in pos.iter_mut().zip(origin) {
        *p = o? * unit;
    }

    let mut atoms = Vec::new();
    for i in 6..6 + atom_lines {
        let f = fields(i)?;
        let z = count(f.first(), i)?;
        if !(0..=118).contains(&z) {
            return Err(err(i + 1, format!("invalid atomic number {z}")));
        }
        let mut p = [0.0; 3];
        for (k, v) in p.iter_mut().enumerate() {
            *v = real(f.get(k + 2), i)? * unit;
        }
        if z > 0 {
            atoms.push(Atom { z: z as u8, pos: p });
        }
    }
    if atoms.is_empty() {
        return Err(err(3, "the file has no atoms".into()));
    }

    let mut data = 6 + atom_lines;
    if natoms < 0 {
        let m = count(fields(data)?.first(), data)?;
        sets = usize::try_from(m).unwrap_or(0);
        let mut left = sets.saturating_add(1);
        while left > 0 {
            left = left.saturating_sub(fields(data)?.len());
            data += 1;
        }
    }
    if sets == 0 {
        return Err(err(data, "invalid number of data sets".into()));
    }
    let points = n[0].checked_mul(n[1]).and_then(|v| v.checked_mul(n[2]));
    let total = points.and_then(|p| p.checked_mul(sets));
    let (Some(points), Some(total)) = (points, total) else {
        return Err(err(4, "grid too large".into()));
    };
    if total > text.len() / 2 {
        return Err(err(4, "the grid is larger than the file".into()));
    }

    let mut values = Vec::with_capacity(points);
    let mut seen = 0;
    'read: for (i, line) in lines.iter().enumerate().skip(data) {
        for tok in line.split_ascii_whitespace() {
            if seen == total {
                break 'read;
            }
            let v = tok
                .parse::<f32>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or_else(|| err(i + 1, format!("invalid grid value '{tok}'")))?;
            if seen % sets == 0 {
                values.push(v);
            }
            seen += 1;
        }
    }
    if seen < total {
        return Err(err(
            lines.len(),
            format!("truncated grid: {seen} of {total} values"),
        ));
    }

    let mut warnings = Vec::new();
    if sets > 1 {
        warnings.push(format!(
            "{}: the file has {sets} data sets; only the first one is shown",
            path.display()
        ));
    }
    let structure = Structure {
        atoms,
        energy: None,
        energy_source: None,
        label: String::new(),
        comment: lines.first().map_or("", |l| l.trim()).to_string(),
        path: path.to_path_buf(),
        line: 1,
        columns: Vec::new(),
        grid: Some(Box::new(Grid {
            n,
            origin: pos,
            axes,
            values,
        })),
    };
    Ok((structure, warnings))
}

impl Grid {
    pub fn max_abs(&self) -> f64 {
        f64::from(self.values.iter().fold(0.0f32, |m, v| m.max(v.abs())))
    }

    pub fn isosurface(&self, iso: f64, step: usize) -> Vec<Face> {
        let step = step.max(1);
        let m = self.n.map(|p| (p - 1) / step);
        if m.contains(&0) {
            return Vec::new();
        }
        let (level, sign) = (iso as f32, iso.signum() as f32);
        let g = |p: [usize; 3]| {
            let i = ((p[0] * step * self.n[1]) + p[1] * step) * self.n[2] + p[2] * step;
            sign * (self.values[i] - level)
        };
        let corner = |p: [usize; 3], c: usize| [0, 1, 2].map(|a| p[a] + (c >> a & 1));
        let cell_index = |p: [usize; 3]| (p[0] * m[1] + p[1]) * m[2] + p[2];

        const NONE: u32 = u32::MAX;
        let mut cells = vec![NONE; m[0] * m[1] * m[2]];
        let mut vertices: Vec<([f64; 3], [f64; 3])> = Vec::new();
        for_each(m.map(|v| v - 1), |p| {
            let values: [f32; 8] = std::array::from_fn(|c| g(corner(p, c)));
            if values.iter().all(|&v| v > 0.0) || values.iter().all(|&v| v <= 0.0) {
                return;
            }
            let (mut sum, mut crossings) = ([0.0; 3], 0.0);
            for c in 0..8 {
                for a in (0..3).filter(|a| c >> a & 1 == 0) {
                    let (v0, v1) = (values[c], values[c | 1 << a]);
                    if (v0 > 0.0) != (v1 > 0.0) {
                        let t = f64::from(v0 / (v0 - v1));
                        for (b, s) in sum.iter_mut().enumerate() {
                            let along = f64::from((c >> b & 1) as u32);
                            *s += if b == a { t } else { along };
                        }
                        crossings += 1.0;
                    }
                }
            }
            cells[cell_index(p)] = vertices.len() as u32;
            let local = [0, 1, 2].map(|a| (p[a] as f64 + sum[a] / crossings) * step as f64);
            let mut normal = [0.0; 3];
            for a in 0..3 {
                let slope: f32 = (0..8)
                    .map(|c| {
                        if c >> a & 1 == 1 {
                            -values[c]
                        } else {
                            values[c]
                        }
                    })
                    .sum();
                let axis = self.axes[a];
                let len2 = axis.iter().map(|v| v * v).sum::<f64>().max(1e-300);
                for (n, v) in normal.iter_mut().zip(axis) {
                    *n += f64::from(slope) * v / len2;
                }
            }
            vertices.push((self.position(local), normal));
        });

        let a = &self.axes;
        let det = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
            - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
            + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
        let mut quads = Vec::with_capacity(vertices.len());
        for_each(m, |p| {
            let v0 = g(p) > 0.0;
            for a in 0..3 {
                let (b, c) = ((a + 1) % 3, (a + 2) % 3);
                if p[a] == m[a] || p[b] == 0 || p[b] == m[b] || p[c] == 0 || p[c] == m[c] {
                    continue;
                }
                let mut next = p;
                next[a] += 1;
                if v0 == (g(next) > 0.0) {
                    continue;
                }
                let around = |db: usize, dc: usize| {
                    let mut q = p;
                    q[b] -= db;
                    q[c] -= dc;
                    cells[cell_index(q)]
                };
                let ids = [around(1, 1), around(0, 1), around(0, 0), around(1, 0)];
                if ids.contains(&NONE) {
                    continue;
                }
                let mut corners = ids.map(|id| vertices[id as usize].0);
                if v0 != (det > 0.0) {
                    corners.swap(1, 3);
                }
                let normal =
                    [0, 1, 2].map(|k| ids.iter().map(|&id| vertices[id as usize].1[k]).sum());
                quads.push(Face { corners, normal });
            }
        });
        quads
    }

    fn position(&self, p: [f64; 3]) -> [f64; 3] {
        [0, 1, 2].map(|k| self.origin[k] + (0..3).map(|a| p[a] * self.axes[a][k]).sum::<f64>())
    }
}

fn for_each(last: [usize; 3], mut f: impl FnMut([usize; 3])) {
    for i in 0..=last[0] {
        for j in 0..=last[1] {
            for k in 0..=last[2] {
                f([i, j, k]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(n: usize, f: impl Fn(f64, f64, f64) -> f64) -> String {
        let o = -((n - 1) as f64) / 2.0;
        let mut s = format!("title\nsecond line\n 1 {o} {o} {o}\n");
        for a in 0..3 {
            let v = [0, 1, 2].map(|k| if k == a { 1 } else { 0 });
            s += &format!("{n} {} {} {}\n", v[0], v[1], v[2]);
        }
        s += "8 8.0 0.0 0.0 0.0\n";
        for i in 0..n {
            for j in 0..n {
                for k in 0..n {
                    let p = [i, j, k].map(|v| v as f64 + o);
                    s += &format!("{:.6E} ", f(p[0], p[1], p[2]));
                }
                s += "\n";
            }
        }
        s
    }

    fn read(text: &str) -> Result<(Structure, Vec<String>), Error> {
        parse(text, Path::new("t.cube"))
    }

    fn mean(q: &[[f64; 3]; 4]) -> [f64; 3] {
        [0, 1, 2].map(|k| q.iter().map(|v| v[k]).sum::<f64>() / 4.0)
    }

    #[test]
    fn reads_atoms_and_grid_in_angstrom() {
        let (s, warnings) = read(&sample(4, |x, _, _| x)).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(s.atoms[0].z, 8);
        assert_eq!(s.energy, None);
        let g = s.grid.unwrap();
        assert_eq!(g.n, [4, 4, 4]);
        assert_eq!(g.values.len(), 64);
        assert!((g.axes[0][0] - BOHR_TO_ANGSTROM).abs() < 1e-12);
        assert!((g.origin[1] + 1.5 * BOHR_TO_ANGSTROM).abs() < 1e-12);
        assert!(g.values[..16].iter().all(|&v| (v + 1.5).abs() < 1e-6));
        assert!((g.values[16] + 0.5).abs() < 1e-6);
    }

    #[test]
    fn negative_counts_mean_angstrom() {
        let text = sample(3, |_, _, _| 1.0).replacen("3 1 0 0", "-3 1 0 0", 1);
        let g = read(&text).unwrap().0.grid.unwrap();
        assert_eq!(g.n, [3, 3, 3]);
        assert_eq!(g.axes[0][0], 1.0);
    }

    #[test]
    fn orbital_cube_keeps_the_first_of_several() {
        let text = "a\nb\n -1 0 0 0\n2 1 0 0\n2 0 1 0\n2 0 0 1\n1 1.0 0 0 0\n2 7 8\n\
                    1 10 2 20 3 30 4 40\n5 50 6 60 7 70 8 80\n";
        let (s, warnings) = read(text).unwrap();
        assert_eq!(s.grid.unwrap().values, [1., 2., 3., 4., 5., 6., 7., 8.]);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn invalid_files_report_their_line() {
        let good = sample(3, |_, _, _| 1.0);
        let huge = good.replacen("3 1 0 0", "2000000000 1 0 0", 1);
        assert!(
            read(&huge)
                .unwrap_err()
                .msg
                .contains("larger than the file")
        );
        let cut = &good[..good.len() - 60];
        assert!(read(cut).unwrap_err().msg.contains("truncated"));
        let bad = good.replacen("8 8.0", "200 8.0", 1);
        assert_eq!(read(&bad).unwrap_err().line, Some(7));
        assert!(read("a\nb\n").is_err());
    }

    #[test]
    fn hostile_input_does_not_panic() {
        crate::mutations(&sample(4, |x, y, z| x * y - z), 2000, |t| {
            if let Ok((s, _)) = read(t) {
                let _ = s.grid.unwrap().isosurface(0.5, 1);
            }
        });
    }

    #[test]
    fn isosurface_of_a_sphere() {
        let radius = 6.0;
        for sign in [1.0, -1.0] {
            let f = move |x: f64, y: f64, z: f64| {
                sign * (-(x * x + y * y + z * z) / (radius * radius)).exp()
            };
            let mut g = read(&sample(24, f)).unwrap().0.grid.unwrap();
            let iso = sign * (-1.0f64).exp();
            for mirrored in [false, true] {
                if mirrored {
                    g.axes[0][0] *= -1.0;
                    g.origin[0] *= -1.0;
                }
                let quads = g.isosurface(iso, 1);
                assert!(quads.len() > 500, "{}", quads.len());
                let r = radius * BOHR_TO_ANGSTROM;
                for face in &quads {
                    let q = &face.corners;
                    for v in q {
                        let d = v.iter().map(|c| c * c).sum::<f64>().sqrt();
                        assert!((d - r).abs() < 0.08, "{d} frente a {r}");
                    }
                    let e = |a: usize, b: usize| [0, 1, 2].map(|k| q[a][k] - q[b][k]);
                    let (u, v) = (e(2, 0), e(3, 1));
                    let n = [
                        u[1] * v[2] - u[2] * v[1],
                        u[2] * v[0] - u[0] * v[2],
                        u[0] * v[1] - u[1] * v[0],
                    ];
                    let c = mean(q);
                    assert!(
                        (0..3).map(|k| n[k] * c[k]).sum::<f64>() > 0.0,
                        "normal hacia dentro"
                    );
                    let m = |v: [f64; 3]| v.iter().map(|x| x * x).sum::<f64>().sqrt();
                    let along = (0..3).map(|k| face.normal[k] * c[k]).sum::<f64>();
                    assert!(
                        along / (m(face.normal) * m(c)) > 0.99,
                        "normal suave no radial"
                    );
                }
                let coarse = g.isosurface(iso, 2).len();
                assert!(
                    coarse * 3 < quads.len(),
                    "{coarse} frente a {}",
                    quads.len()
                );
            }
        }
    }
}
