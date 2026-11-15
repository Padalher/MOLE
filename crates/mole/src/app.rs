use mole_core::cube::Face;
use mole_core::{DEFAULT_BOND_TOLERANCE, Loaded, Structure, bonds, elements};

type Mat3 = [[f64; 3]; 3];

const IDENTITY: Mat3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

fn mul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut m = [[0.0; 3]; 3];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    m
}

fn apply(m: &Mat3, v: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|i| m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2])
}

fn rotation(ax: f64, ay: f64) -> Mat3 {
    let (sx, cx) = ax.sin_cos();
    let (sy, cy) = ay.sin_cos();
    mul(
        &[[1.0, 0.0, 0.0], [0.0, cx, -sx], [0.0, sx, cx]],
        &[[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]],
    )
}

fn transpose(m: &Mat3) -> Mat3 {
    [0, 1, 2].map(|i| [m[0][i], m[1][i], m[2][i]])
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn sym_eigen<const N: usize>(mut a: [[f64; N]; N]) -> ([f64; N], [[f64; N]; N]) {
    let mut v: [[f64; N]; N] = std::array::from_fn(|i| std::array::from_fn(|j| f64::from(i == j)));
    for _ in 0..60 {
        let off: f64 = (0..N)
            .flat_map(|i| (i + 1..N).map(move |j| (i, j)))
            .map(|(i, j)| a[i][j] * a[i][j])
            .sum();
        if off < 1e-30 {
            break;
        }
        for p in 0..N {
            for q in p + 1..N {
                if a[p][q] == 0.0 {
                    continue;
                }
                let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
                let t = theta.signum() / (theta.abs() + theta.hypot(1.0));
                let c = 1.0 / t.hypot(1.0);
                let s = t * c;
                for row in a.iter_mut() {
                    let (kp, kq) = (row[p], row[q]);
                    (row[p], row[q]) = (c * kp - s * kq, s * kp + c * kq);
                }
                let (rp, rq) = (a[p], a[q]);
                a[p] = std::array::from_fn(|k| c * rp[k] - s * rq[k]);
                a[q] = std::array::from_fn(|k| s * rp[k] + c * rq[k]);
                for row in v.iter_mut() {
                    let (kp, kq) = (row[p], row[q]);
                    (row[p], row[q]) = (c * kp - s * kq, s * kp + c * kq);
                }
            }
        }
    }
    (std::array::from_fn(|i| a[i][i]), v)
}

fn centered(s: &Structure) -> Vec<[f64; 3]> {
    let n = s.atoms.len().max(1) as f64;
    let c = [0, 1, 2].map(|k| s.atoms.iter().map(|a| a.pos[k]).sum::<f64>() / n);
    s.atoms
        .iter()
        .map(|a| [0, 1, 2].map(|k| a.pos[k] - c[k]))
        .collect()
}

fn superpose(p: &[[f64; 3]], q: &[[f64; 3]]) -> Mat3 {
    let mut s = [[0.0; 3]; 3];
    for (a, b) in p.iter().zip(q) {
        for i in 0..3 {
            for j in 0..3 {
                s[i][j] += a[i] * b[j];
            }
        }
    }
    let [[xx, xy, xz], [yx, yy, yz], [zx, zy, zz]] = s;
    let (vals, vecs) = sym_eigen([
        [xx + yy + zz, yz - zy, zx - xz, xy - yx],
        [yz - zy, xx - yy - zz, xy + yx, zx + xz],
        [zx - xz, xy + yx, -xx + yy - zz, yz + zy],
        [xy - yx, zx + xz, yz + zy, -xx - yy + zz],
    ]);
    let k = (0..4)
        .max_by(|&a, &b| vals[a].total_cmp(&vals[b]))
        .unwrap_or(0);
    let [w, x, y, z] = [0, 1, 2, 3].map(|i| vecs[i][k]);
    [
        [
            w * w + x * x - y * y - z * z,
            2.0 * (x * y - w * z),
            2.0 * (x * z + w * y),
        ],
        [
            2.0 * (x * y + w * z),
            w * w - x * x + y * y - z * z,
            2.0 * (y * z - w * x),
        ],
        [
            2.0 * (x * z - w * y),
            2.0 * (y * z + w * x),
            w * w - x * x - y * y + z * z,
        ],
    ]
}

fn principal_axes(s: &Structure) -> Mat3 {
    let p = centered(s);
    let w: Vec<f64> = s.atoms.iter().map(|a| f64::from(a.z)).collect();
    let mut g = [[0.0; 3]; 3];
    for (r, m) in p.iter().zip(&w) {
        for i in 0..3 {
            for j in 0..3 {
                g[i][j] += m * r[i] * r[j];
            }
        }
    }
    let (vals, vecs) = sym_eigen(g);
    let mut order = [0, 1, 2];
    order.sort_by(|&a, &b| vals[b].total_cmp(&vals[a]));
    let mut e: Mat3 = order.map(|k| [0, 1, 2].map(|i| vecs[i][k]));
    for ax in e.iter_mut().take(2) {
        let skew: f64 = p
            .iter()
            .zip(&w)
            .map(|(r, m)| m * dot(*ax, *r).powi(3))
            .sum();
        if skew < 0.0 {
            *ax = ax.map(|v| -v);
        }
    }
    e[2] = cross(e[0], e[1]);
    e
}

fn orientation_to(s: &Structure, r: &Structure) -> Mat3 {
    let (p, q) = (centered(s), centered(r));
    let (es, er) = (principal_axes(s), principal_axes(r));
    let mut candidates: Vec<Mat3> = [(1.0, 1.0), (-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0)]
        .iter()
        .map(|&(a, b)| {
            let (e0, e1) = (es[0].map(|v| v * a), es[1].map(|v| v * b));
            mul(&transpose(&er), &[e0, e1, cross(e0, e1)])
        })
        .collect();
    if s.atoms.len() == r.atoms.len() && s.atoms.iter().zip(&r.atoms).all(|(a, b)| a.z == b.z) {
        candidates.push(superpose(&p, &q));
    }
    let mismatch = |m: &Mat3| -> f64 {
        p.iter()
            .zip(&s.atoms)
            .map(|(&x, a)| {
                let y = apply(m, x);
                q.iter()
                    .zip(&r.atoms)
                    .map(|(&t, b)| {
                        let d = [0, 1, 2].map(|k| y[k] - t[k]);
                        dot(d, d) + if a.z == b.z { 0.0 } else { 100.0 }
                    })
                    .fold(f64::INFINITY, f64::min)
            })
            .sum()
    };
    candidates
        .into_iter()
        .map(|m| (mismatch(&m), m))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map_or(IDENTITY, |(_, m)| m)
}

pub struct Player {
    pub len: usize,
    pub current: usize,
    pub playing: bool,
    pub looping: bool,
    pub speed: f64,
    last_step: f64,
}

pub const SPEEDS: [f64; 9] = [0.5, 1.0, 2.0, 4.0, 8.0, 15.0, 30.0, 60.0, 120.0];

impl Player {
    pub fn new(len: usize) -> Self {
        Player {
            len,
            current: 0,
            playing: false,
            looping: true,
            speed: 4.0,
            last_step: 0.0,
        }
    }

    pub fn go(&mut self, i: usize) {
        self.current = i.min(self.len - 1);
    }

    pub fn next(&mut self) {
        if self.current + 1 < self.len {
            self.current += 1;
        } else if self.looping {
            self.current = 0;
        } else {
            self.playing = false;
        }
    }

    pub fn prev(&mut self) {
        self.current = match self.current {
            0 => self.len - 1,
            c => c - 1,
        };
    }

    pub fn toggle(&mut self, now: f64) {
        self.playing = !self.playing;
        self.last_step = now;
        if self.playing && !self.looping && self.current + 1 == self.len {
            self.current = 0;
        }
    }

    pub fn tick(&mut self, now: f64) -> Option<f64> {
        if !self.playing || self.len < 2 {
            return None;
        }
        let period = 1.0 / self.speed;
        if now - self.last_step >= period {
            self.next();
            self.last_step = now;
        }
        self.playing
            .then(|| (period - (now - self.last_step)).max(0.0))
    }

    fn change_speed(&mut self, up: bool) {
        let i = SPEEDS.iter().position(|&s| s >= self.speed).unwrap_or(0);
        let i = if up {
            (i + 1).min(SPEEDS.len() - 1)
        } else {
            i.saturating_sub(1)
        };
        self.speed = SPEEDS[i];
    }
}

pub fn nearest_with_energy(structures: &[Structure], x: f64) -> Option<usize> {
    structures
        .iter()
        .enumerate()
        .filter(|(_, s)| s.energy.is_some())
        .min_by(|(a, _), (b, _)| {
            let d = |i: &usize| ((*i + 1) as f64 - x).abs();
            d(a).total_cmp(&d(b))
        })
        .map(|(i, _)| i)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    First,
    Prev,
    Play,
    Next,
    Last,
    Slower,
    Faster,
    Loop,
    LessTolerance,
    MoreTolerance,
    MetalMetal,
    Smooth,
    RelativeEnergy,
    FixOrientation,
    Light,
    LessIso,
    MoreIso,
    Transparent,
    Forces,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BarItem {
    Button(Button),
    Speed,
    Tolerance,
    Iso,
    Title(&'static str),
}

#[derive(Debug, Clone, Default)]
pub struct Layout {
    pub w: i32,
    pub h: i32,
    pub char_w: i32,
    pub line_h: i32,
    pub header: Rect,
    pub view: Rect,
    pub plot: Rect,
    pub panel: Rect,
    pub items: Vec<(BarItem, Rect)>,
    pub panes: Panes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Panes {
    pub plot_h: Option<i32>,
    pub panel_w: Option<i32>,
    pub cube: bool,
    pub forces: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Plot,
    Panel,
}

pub fn speed_text(speed: f64) -> String {
    format!("{speed:>3} struct/s")
}

pub fn tolerance_text(t: f64) -> String {
    format!("tol {t:.2}")
}

pub const METAL_METAL_LABEL: &str = "M-M bonds";
pub const RELATIVE_ENERGY_LABEL: &str = "E - Emin";
pub const FIX_ORIENTATION_LABEL: &str = "Fix orientation";
pub const LIGHT_LABEL: &str = "Light theme";
pub const TRANSPARENT_LABEL: &str = "Transparent";
pub const FORCES_LABEL: &str = "Arrows";

pub fn iso_text(iso: f64) -> String {
    format!("iso {iso:.4}")
}

pub fn playback_label(b: Button, playing: bool) -> Option<&'static str> {
    Some(match b {
        Button::First => "First",
        Button::Prev => "Previous",
        Button::Play if playing => "Pause",
        Button::Play => "Play",
        Button::Next => "Next",
        Button::Last => "Last",
        _ => return None,
    })
}
const PLAYBACK_LABEL_MAX: i32 = "Previous".len() as i32;

const MIN_PANE_H: i32 = 80;

impl Layout {
    pub fn new(w: i32, h: i32, char_w: i32, line_h: i32) -> Self {
        Self::with_panes(w, h, char_w, line_h, Panes::default())
    }

    pub fn with_panes(w: i32, h: i32, char_w: i32, line_h: i32, panes: Panes) -> Self {
        let header = Rect {
            x: 0,
            y: 0,
            w,
            h: line_h + 10,
        };
        let (bh, gap, pad) = ((2 * line_h + 4).max(34), 4, 6);
        let text_w = char_w * speed_text(0.0).len() as i32 + 4;
        let inner = (2 * bh + 2 * gap + text_w).max(bh + char_w * (PLAYBACK_LABEL_MAX + 2));
        let min_panel = inner + 2 * pad;
        let panel_w = panes.panel_w.unwrap_or(0).min(w / 2).max(min_panel).min(w);
        let panel = Rect {
            x: w - panel_w,
            y: header.h,
            w: panel_w,
            h: (h - header.h).max(0),
        };
        let left = w - panel_w;
        let plot_h = panes.plot_h.unwrap_or((h / 3).clamp(120, 280));
        let plot_h = plot_h.min(h - header.h - MIN_PANE_H).max(MIN_PANE_H);
        let plot = Rect {
            x: 0,
            y: h - plot_h,
            w: left,
            h: plot_h,
        };
        let view = Rect {
            x: 0,
            y: header.h,
            w: left,
            h: (plot.y - header.h).max(0),
        };
        let (x0, x1) = (panel.x + pad, panel.x + panel_w - pad);
        let value_w = x1 - x0 - 2 * (bh + gap);
        enum Row {
            Full(Button),
            Step(Button, BarItem, Button),
        }
        use Row::{Full, Step};
        let mut groups = vec![
            (
                "PLAYBACK",
                vec![
                    Full(Button::First),
                    Full(Button::Last),
                    Full(Button::Play),
                    Full(Button::Prev),
                    Full(Button::Next),
                    Step(Button::Slower, BarItem::Speed, Button::Faster),
                    Full(Button::Loop),
                ],
            ),
            (
                "BONDS",
                vec![
                    Step(
                        Button::LessTolerance,
                        BarItem::Tolerance,
                        Button::MoreTolerance,
                    ),
                    Full(Button::MetalMetal),
                ],
            ),
        ];
        if panes.cube {
            groups.push((
                "SURFACE",
                vec![
                    Step(Button::LessIso, BarItem::Iso, Button::MoreIso),
                    Full(Button::Transparent),
                ],
            ));
        }
        if panes.forces {
            groups.push(("FORCES", vec![Full(Button::Forces)]));
        }
        groups.push((
            "VIEW",
            vec![
                Full(Button::FixOrientation),
                Full(Button::Smooth),
                Full(Button::Light),
            ],
        ));
        groups.push(("PLOT", vec![Full(Button::RelativeEnergy)]));
        let mut items = Vec::new();
        let mut put =
            |item, x: i32, w: i32, y: i32, h: i32| items.push((item, Rect { x, y, w, h }));
        let mut y = panel.y + pad;
        for (k, (name, rows)) in groups.into_iter().enumerate() {
            if k > 0 {
                y += 8;
            }
            put(BarItem::Title(name), x0, x1 - x0, y, line_h);
            y += line_h + 2;
            for row in rows {
                match row {
                    Full(b) => put(BarItem::Button(b), x0, x1 - x0, y, bh),
                    Step(less, value, more) => {
                        put(BarItem::Button(less), x0, bh, y, bh);
                        put(value, x0 + bh + gap, value_w, y, bh);
                        put(BarItem::Button(more), x1 - bh, bh, y, bh);
                    }
                }
                y += bh + gap;
            }
        }
        Layout {
            w,
            h,
            char_w,
            line_h,
            header,
            view,
            plot,
            panel,
            items,
            panes: Panes {
                plot_h: panes.plot_h.map(|_| plot_h),
                panel_w: panes.panel_w.map(|_| panel_w),
                cube: panes.cube,
                forces: panes.forces,
            },
        }
    }

    pub fn edge_at(&self, x: i32, y: i32) -> Option<Edge> {
        if y > self.header.h && (x - self.panel.x).abs() <= 3 {
            Some(Edge::Panel)
        } else if x < self.plot.w && (y - self.plot.y).abs() <= 4 {
            Some(Edge::Plot)
        } else {
            None
        }
    }

    pub fn button_at(&self, x: i32, y: i32) -> Option<Button> {
        self.items.iter().find_map(|&(item, r)| match item {
            BarItem::Button(b) if r.contains(x, y) => Some(b),
            _ => None,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PlotMap {
    pub area: Rect,
    pub n: (f64, f64),
    pub e: (f64, f64),
    pub e_ref: Option<f64>,
}

impl PlotMap {
    pub fn x(&self, n: f64) -> f64 {
        f64::from(self.area.x)
            + f64::from(self.area.w) * (n - self.n.0) / (self.n.1 - self.n.0).max(1e-12)
    }

    pub fn y(&self, e: f64) -> f64 {
        f64::from(self.area.y)
            + f64::from(self.area.h) * (self.e.1 - e) / (self.e.1 - self.e.0).max(1e-300)
    }

    pub fn n_at(&self, x: i32) -> f64 {
        self.n.0
            + (self.n.1 - self.n.0) * f64::from(x - self.area.x) / f64::from(self.area.w.max(1))
    }
}

pub fn nice_ticks(lo: f64, hi: f64, max: usize) -> (Vec<f64>, f64) {
    let span = (hi - lo).abs().max(1e-12);
    let raw = span / max.max(1) as f64;
    let mag = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0]
        .iter()
        .map(|m| m * mag)
        .find(|&s| s >= raw)
        .unwrap_or(10.0 * mag);
    let first = (lo / step).ceil() as i64;
    let last = (hi / step).floor() as i64;
    ((first..=last).map(|k| k as f64 * step).collect(), step)
}

pub fn tick_text(v: f64, step: f64) -> String {
    let decimals = (-step.log10().floor()).max(0.0) as usize;
    format!("{v:.decimals$}")
}

pub fn tick_exponent(ticks: &[f64], step: f64) -> i32 {
    let k = step.log10().floor() as i32;
    let big = ticks.iter().fold(0.0f64, |m, t| m.max(t.abs()));
    if k >= -3 || big / 10f64.powi(k) >= 1e5 {
        0
    } else {
        k
    }
}

pub fn short_number(v: f64) -> String {
    if v != 0.0 && v.abs() < 1e-3 {
        return format!("{v:.2e}");
    }
    let s = format!("{v:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Hover {
    #[default]
    None,
    Atom(usize),
    Plot(usize),
    Button(Button),
    Edge(Edge),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Left,
    Right,
    Home,
    End,
    Space,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Input {
    Press { x: i32, y: i32, button: u8 },
    Release { button: u8 },
    Motion { x: i32, y: i32 },
    Key(Key),
    Resize { w: i32, h: i32 },
    Leave,
}

type SurfaceFace = (Face, bool);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape3d {
    Sphere {
        x: f64,
        y: f64,
        r: f64,
        rgb: [f64; 3],
        fade: f64,
    },
    Bond {
        from: (f64, f64),
        to: (f64, f64),
        width: f64,
        rgb: [f64; 3],
        fade: f64,
    },
    Quad {
        pts: [(f64, f64); 4],
        rgb: [f64; 3],
    },
    Arrow {
        from: (f64, f64),
        to: (f64, f64),
        head: [(f64, f64); 3],
    },
}

pub fn element_rgb(z: u8) -> [f64; 3] {
    let c: [u8; 3] = match z {
        1 => [235, 235, 235],
        5 => [255, 170, 150],
        6 => [90, 90, 90],
        7 => [60, 90, 225],
        8 => [225, 45, 40],
        9 | 17 => [70, 200, 70],
        15 => [240, 140, 30],
        16 => [235, 210, 50],
        35 => [160, 50, 40],
        53 => [140, 40, 160],
        13 => [195, 165, 175],
        14 => [210, 180, 140],
        22 => [140, 160, 200],
        26 => [215, 110, 50],
        29 => [200, 120, 70],
        47 => [200, 200, 210],
        79 => [235, 190, 40],
        z => ELEMENT_COLORS
            .get(usize::from(z).wrapping_sub(1))
            .copied()
            .unwrap_or([200, 120, 200]),
    };
    c.map(|v| f64::from(v) / 255.0)
}

const ELEMENT_COLORS: [[u8; 3]; 118] = [
    [255, 255, 255],
    [217, 255, 255],
    [204, 128, 255],
    [194, 255, 0],
    [255, 181, 181],
    [128, 128, 128],
    [13, 13, 255],
    [255, 13, 13],
    [178, 255, 255],
    [178, 227, 245],
    [171, 92, 242],
    [138, 255, 0],
    [191, 166, 166],
    [128, 153, 153],
    [255, 128, 0],
    [255, 255, 48],
    [31, 240, 31],
    [128, 209, 227],
    [143, 64, 212],
    [61, 255, 0],
    [230, 230, 230],
    [191, 194, 199],
    [166, 166, 171],
    [138, 153, 199],
    [156, 122, 199],
    [128, 122, 199],
    [112, 122, 199],
    [92, 122, 194],
    [255, 122, 97],
    [125, 128, 176],
    [194, 143, 143],
    [102, 143, 143],
    [189, 128, 227],
    [255, 161, 0],
    [166, 41, 41],
    [92, 184, 209],
    [112, 46, 176],
    [0, 255, 0],
    [148, 255, 255],
    [148, 224, 224],
    [115, 194, 201],
    [84, 181, 181],
    [59, 158, 158],
    [36, 143, 143],
    [10, 125, 140],
    [0, 105, 133],
    [224, 224, 255],
    [255, 217, 143],
    [166, 117, 115],
    [102, 128, 128],
    [158, 99, 181],
    [212, 122, 0],
    [148, 0, 148],
    [66, 158, 176],
    [87, 23, 143],
    [0, 201, 0],
    [112, 212, 255],
    [255, 255, 199],
    [217, 255, 199],
    [199, 255, 199],
    [163, 255, 199],
    [143, 255, 199],
    [97, 255, 199],
    [69, 255, 199],
    [48, 255, 199],
    [31, 255, 199],
    [0, 255, 156],
    [0, 230, 117],
    [0, 212, 82],
    [0, 191, 56],
    [0, 171, 36],
    [76, 194, 255],
    [76, 166, 255],
    [33, 148, 214],
    [38, 125, 171],
    [38, 102, 150],
    [23, 84, 135],
    [245, 237, 209],
    [204, 209, 31],
    [181, 181, 194],
    [166, 84, 76],
    [87, 89, 97],
    [158, 79, 181],
    [171, 92, 0],
    [117, 79, 69],
    [66, 130, 150],
    [66, 0, 102],
    [0, 125, 0],
    [112, 171, 250],
    [0, 186, 255],
    [0, 161, 255],
    [0, 143, 255],
    [0, 128, 255],
    [0, 107, 255],
    [84, 92, 242],
    [120, 92, 227],
    [138, 79, 227],
    [161, 54, 212],
    [178, 31, 212],
    [178, 31, 186],
    [178, 13, 166],
    [189, 13, 135],
    [199, 0, 102],
    [204, 0, 89],
    [209, 0, 79],
    [217, 0, 69],
    [224, 0, 56],
    [230, 0, 46],
    [232, 0, 38],
    [235, 0, 36],
    [237, 0, 33],
    [240, 0, 31],
    [242, 0, 28],
    [245, 0, 26],
    [247, 0, 23],
    [250, 0, 20],
    [252, 0, 18],
    [255, 0, 15],
];

fn ball_radius(z: u8) -> f64 {
    0.2 + 0.3 * elements::covalent_radius(z)
}

const BOND_RADIUS: f64 = 0.12;

const DEFAULT_ISO: f64 = 0.02;
const MIN_ISO: f64 = 1e-4;
const MAX_ISO: f64 = 10.0;
const ISO_STEP: f64 = 1.25;
const MAX_QUADS: usize = 6000;
const LOBE_RGB: [[f64; 3]; 2] = [[0.86, 0.16, 0.16], [0.16, 0.32, 0.9]];
const FORCE_LEN: f64 = 1.5;

fn forces(s: &Structure) -> Option<&[f64]> {
    s.column("forces")
        .filter(|c| c.width == 3)
        .map(|c| c.values.as_slice())
}

fn max_force(f: &[f64]) -> f64 {
    f.chunks(3)
        .map(norm)
        .filter(|n| n.is_finite())
        .fold(0.0, f64::max)
}

fn norm(v: &[f64]) -> f64 {
    v.iter().map(|c| c * c).sum::<f64>().sqrt()
}

pub fn column_lines(s: &Structure, i: usize) -> Vec<String> {
    let parts: Vec<String> = s
        .columns
        .iter()
        .filter_map(|c| {
            let v = c.values.get(i * c.width..(i + 1) * c.width)?;
            let text: Vec<String> = v.iter().map(|&x| short_number(x)).collect();
            Some(match v.len() {
                1 => format!("{}={}", c.name, text[0]),
                _ if c.name == "forces" => {
                    format!("forces=({}) |F|={}", text.join(", "), short_number(norm(v)))
                }
                _ => format!("{}=({})", c.name, text.join(", ")),
            })
        })
        .collect();
    parts.chunks(4).map(|c| c.join("  ")).collect()
}

pub struct App {
    pub loaded: Loaded,
    pub player: Player,
    pub bond_tolerance: f64,
    pub metal_metal: bool,
    pub relative_energy: bool,
    pub iso: f64,
    pub transparent: bool,
    pub light: bool,
    pub forces: bool,
    pub fixed_orientation: Option<Vec<Mat3>>,
    pub smooth: bool,
    pub layout: Layout,
    pub hover: Hover,
    pub pointer: (i32, i32),
    pub quit: bool,
    rotation: Mat3,
    zoom: f64,
    pan: (f64, f64),
    extent: f64,
    drag: Option<(u8, i32, i32)>,
    scrub: bool,
    resizing: Option<Edge>,
    surface: (Option<(usize, u64)>, Vec<SurfaceFace>),
    last_click: Option<(f64, i32, i32)>,
    bonds: BondCache,
}

type BondCache = (Option<(usize, f64, bool)>, Vec<(usize, usize)>);

impl App {
    pub fn new(loaded: Loaded, layout: Layout) -> Self {
        let grids = || loaded.structures.iter().filter_map(|s| s.grid.as_deref());
        let iso = grids().next().map_or(DEFAULT_ISO, |g| {
            (0.1 * g.max_abs()).clamp(MIN_ISO, DEFAULT_ISO)
        });
        let no_energy = loaded
            .structures
            .iter()
            .filter(|s| s.energy.is_some())
            .count()
            < 2;
        let panes = Panes {
            cube: grids().next().is_some(),
            forces: loaded.structures.iter().any(|s| forces(s).is_some()),
            plot_h: no_energy.then_some(MIN_PANE_H).or(layout.panes.plot_h),
            ..layout.panes
        };
        let layout = Layout::with_panes(layout.w, layout.h, layout.char_w, layout.line_h, panes);
        let extent = loaded
            .structures
            .iter()
            .map(|s| {
                let n = s.atoms.len() as f64;
                let c = [0, 1, 2].map(|k| s.atoms.iter().map(|a| a.pos[k]).sum::<f64>() / n);
                s.atoms
                    .iter()
                    .map(|a| {
                        (0..3)
                            .map(|k| (a.pos[k] - c[k]).powi(2))
                            .sum::<f64>()
                            .sqrt()
                    })
                    .fold(0.0, f64::max)
            })
            .fold(0.0, f64::max)
            + 1.0;
        App {
            player: Player::new(loaded.structures.len()),
            loaded,
            bond_tolerance: DEFAULT_BOND_TOLERANCE,
            metal_metal: true,
            relative_energy: true,
            fixed_orientation: None,
            smooth: false,
            layout,
            hover: Hover::None,
            pointer: (0, 0),
            quit: false,
            rotation: IDENTITY,
            zoom: 1.0,
            pan: (0.0, 0.0),
            extent,
            drag: None,
            scrub: false,
            resizing: None,
            surface: (None, Vec::new()),
            iso,
            forces: true,
            transparent: false,
            light: false,
            last_click: None,
            bonds: (None, Vec::new()),
        }
    }

    pub fn structure(&self) -> &Structure {
        &self.loaded.structures[self.player.current]
    }

    fn current_bonds(&mut self) -> &[(usize, usize)] {
        let key = (self.player.current, self.bond_tolerance, self.metal_metal);
        if self.bonds.0 != Some(key) {
            let atoms = &self.loaded.structures[key.0].atoms;
            self.bonds = (Some(key), bonds(atoms, key.1, key.2));
        }
        &self.bonds.1
    }

    pub fn plot_map(&self) -> Option<PlotMap> {
        let mut es = self.loaded.structures.iter().filter_map(|s| s.energy);
        let first = es.next()?;
        let (lo, hi) = es.fold((first, first), |(lo, hi), e| (lo.min(e), hi.max(e)));
        let pad = if hi > lo { (hi - lo) * 0.06 } else { 1.0 };
        let (cw, lh) = (self.layout.char_w, self.layout.line_h);
        let p = self.layout.plot;
        let left = cw * 13;
        let e_ref = self.relative_energy.then_some(lo);
        let top = lh / 2 + 6 + if e_ref.is_some() { lh } else { 0 };
        let area = Rect {
            x: p.x + left,
            y: p.y + top,
            w: (p.w - left - cw * 3).max(10),
            h: (p.h - top - 3 * lh / 2 - 8).max(10),
        };
        let n = self.loaded.structures.len() as f64;
        Some(PlotMap {
            area,
            n: (0.5, n + 0.5),
            e: (lo - pad, hi + pad),
            e_ref,
        })
    }

    fn view(&self) -> (Mat3, [f64; 3], f64, (f64, f64)) {
        let atoms = &self.structure().atoms;
        let v = self.layout.view;
        let n = atoms.len().max(1) as f64;
        let c = [0, 1, 2].map(|k| atoms.iter().map(|a| a.pos[k]).sum::<f64>() / n);
        let scale = self.zoom * f64::from(v.w.min(v.h)) * 0.45 / self.extent.max(1.0);
        let origin = (
            f64::from(v.x) + f64::from(v.w) / 2.0 + self.pan.0,
            f64::from(v.y) + f64::from(v.h) / 2.0 + self.pan.1,
        );
        let m = match &self.fixed_orientation {
            Some(fixed) => mul(&self.rotation, &fixed[self.player.current]),
            None => self.rotation,
        };
        (m, c, scale, origin)
    }

    fn projection(&self) -> (Vec<[f64; 3]>, f64) {
        let (m, c, scale, (ox, oy)) = self.view();
        let pts = self
            .structure()
            .atoms
            .iter()
            .map(|a| {
                let p = apply(&m, [0, 1, 2].map(|k| a.pos[k] - c[k]));
                [ox + p[0] * scale, oy - p[1] * scale, p[2]]
            })
            .collect();
        (pts, scale)
    }

    fn update_surface(&mut self) {
        let key = (self.player.current, self.iso.to_bits());
        if self.surface.0 == Some(key) {
            return;
        }
        let quads = match &self.loaded.structures[key.0].grid {
            Some(grid) => {
                let (iso, n) = (self.iso, grid.n.iter().copied().max().unwrap_or(1));
                let lobes = |step: usize| -> Vec<_> {
                    let positive = grid.isosurface(iso, step).into_iter().map(|q| (q, true));
                    let negative = grid.isosurface(-iso, step).into_iter().map(|q| (q, false));
                    positive.chain(negative).collect()
                };
                let step = n / 150 + 1;
                let quads = lobes(step);
                if quads.len() > MAX_QUADS {
                    let ratio = (quads.len() as f64 / MAX_QUADS as f64).sqrt();
                    lobes((step as f64 * ratio).ceil() as usize)
                } else {
                    quads
                }
            }
            None => Vec::new(),
        };
        self.surface = (Some(key), quads);
    }

    pub fn scene(&mut self) -> Vec<Shape3d> {
        let bonds = self.current_bonds().to_vec();
        self.update_surface();
        let (p, scale) = self.projection();
        let atoms = &self.structure().atoms;
        let (zmin, zmax) = p.iter().fold((f64::MAX, f64::MIN), |(lo, hi), q| {
            (lo.min(q[2]), hi.max(q[2]))
        });
        let fade = |z: f64| 1.0 - 0.3 * ((zmax - z) / (zmax - zmin).max(1e-9)).clamp(0.0, 1.0);
        let mut items: Vec<(f64, Shape3d)> = atoms
            .iter()
            .zip(&p)
            .map(|(a, q)| {
                let shape = Shape3d::Sphere {
                    x: q[0],
                    y: q[1],
                    r: ball_radius(a.z) * scale,
                    rgb: element_rgb(a.z),
                    fade: fade(q[2]),
                };
                (q[2], shape)
            })
            .collect();
        for (a, b) in bonds {
            for (at, other) in [(a, b), (b, a)] {
                let (s, o) = (p[at], p[other]);
                let d = [o[0] - s[0], o[1] - s[1], (o[2] - s[2]) * scale];
                let half = 0.5 * d.iter().map(|c| c * c).sum::<f64>().sqrt();
                let r = ball_radius(atoms[at].z);
                let r = (r * r - BOND_RADIUS * BOND_RADIUS).max(0.0).sqrt() * scale;
                if half <= r {
                    continue;
                }
                let f = r / (2.0 * half);
                let start = [0, 1, 2].map(|k| s[k] + (o[k] - s[k]) * f);
                let mid = [0, 1, 2].map(|k| 0.5 * (s[k] + o[k]));
                let shape = Shape3d::Bond {
                    from: (start[0], start[1]),
                    to: (mid[0], mid[1]),
                    width: 2.0 * BOND_RADIUS * scale,
                    rgb: element_rgb(atoms[at].z),
                    fade: fade(0.5 * (start[2] + mid[2])),
                };
                items.push((0.5 * (start[2] + mid[2]), shape));
            }
        }
        let (m, c, scale, (ox, oy)) = self.view();
        if let Some(f) = forces(self.structure()).filter(|_| self.forces) {
            let max = max_force(f);
            for (q, v) in p.iter().zip(f.chunks(3)) {
                let r = apply(&m, [v[0], v[1], v[2]]);
                let k = FORCE_LEN * scale / max;
                let (dx, dy) = (r[0] * k, -r[1] * k);
                let len = dx.hypot(dy);
                if len.is_nan() || len < 4.0 {
                    continue;
                }
                let (ux, uy) = (dx / len, dy / len);
                let h = (0.35 * len).min(12.0);
                let tip = (q[0] + dx, q[1] + dy);
                let base = (tip.0 - ux * h, tip.1 - uy * h);
                let side = |s: f64| (base.0 - uy * s * h * 0.45, base.1 + ux * s * h * 0.45);
                let shape = Shape3d::Arrow {
                    from: (q[0], q[1]),
                    to: base,
                    head: [tip, side(1.0), side(-1.0)],
                };
                items.push((f64::INFINITY, shape));
            }
        }
        for (face, positive) in &self.surface.1 {
            let v = face
                .corners
                .map(|q| apply(&m, [0, 1, 2].map(|k| q[k] - c[k])));
            let diag = |a: usize, b: usize| [0, 1, 2].map(|k| v[a][k] - v[b][k]);
            if cross(diag(2, 0), diag(3, 1))[2] <= 0.0 {
                continue;
            }
            let n = apply(&m, face.normal);
            let z = v.iter().map(|p| p[2]).sum::<f64>() / 4.0;
            let lit = dot(n, [-0.4, 0.5, 0.77]).max(0.0) / dot(n, n).sqrt().max(1e-12);
            let k = ((0.3 + 0.75 * lit) * fade(z) * 16.0).round() / 16.0;
            let shape = Shape3d::Quad {
                pts: v.map(|p| (ox + p[0] * scale, oy - p[1] * scale)),
                rgb: LOBE_RGB[usize::from(!positive)].map(|c| (c * k).min(1.0)),
            };
            items.push((z, shape));
        }
        items.sort_by(|x, y| x.0.total_cmp(&y.0));
        items.into_iter().map(|(_, s)| s).collect()
    }

    fn atom_at(&self, x: i32, y: i32) -> Option<usize> {
        let (p, scale) = self.projection();
        let atoms = &self.structure().atoms;
        (0..atoms.len())
            .filter(|&i| {
                let r = ball_radius(atoms[i].z) * scale;
                (p[i][0] - f64::from(x)).hypot(p[i][1] - f64::from(y)) <= r
            })
            .max_by(|&a, &b| p[a][2].total_cmp(&p[b][2]))
    }

    pub fn configure(&mut self, o: &crate::cli::Options) {
        self.smooth = o.smooth || o.transparent;
        self.light = o.light;
        self.relative_energy = !o.absolute;
        self.metal_metal = !o.no_mm;
        self.transparent = o.transparent;
        self.forces &= !o.no_forces;
        if let Some(t) = o.tol {
            self.bond_tolerance = t.clamp(0.8, 1.6);
        }
        if let Some(v) = o.iso {
            self.iso = v.clamp(MIN_ISO, MAX_ISO);
        }
        if let Some(n) = o.frame {
            self.player.go(n - 1);
        }
        if o.fix_orientation {
            self.press(Button::FixOrientation, 0.0);
        }
    }

    fn hover_at(&self, x: i32, y: i32) -> Hover {
        let l = &self.layout;
        if let Some(b) = l.button_at(x, y) {
            Hover::Button(b)
        } else if let Some(edge) = l.edge_at(x, y) {
            Hover::Edge(edge)
        } else if l.view.contains(x, y) {
            self.atom_at(x, y).map_or(Hover::None, Hover::Atom)
        } else if l.plot.contains(x, y)
            && let Some(m) = self.plot_map()
            && m.area.x - 4 <= x
            && x <= m.area.x + m.area.w + 4
        {
            nearest_with_energy(&self.loaded.structures, m.n_at(x)).map_or(Hover::None, Hover::Plot)
        } else {
            Hover::None
        }
    }

    fn scrub_to(&mut self, x: i32) {
        if let Some(m) = self.plot_map() {
            let x = x.clamp(m.area.x, m.area.x + m.area.w);
            if let Some(i) = nearest_with_energy(&self.loaded.structures, m.n_at(x)) {
                self.player.go(i);
            }
        }
    }

    fn press(&mut self, b: Button, now: f64) {
        let p = &mut self.player;
        match b {
            Button::First => p.go(0),
            Button::Prev => p.prev(),
            Button::Play => p.toggle(now),
            Button::Next => p.next(),
            Button::Last => p.go(usize::MAX),
            Button::Slower => p.change_speed(false),
            Button::Faster => p.change_speed(true),
            Button::Loop => p.looping = !p.looping,
            Button::LessTolerance => {
                self.bond_tolerance = (self.bond_tolerance - 0.05).max(0.8);
            }
            Button::MoreTolerance => {
                self.bond_tolerance = (self.bond_tolerance + 0.05).min(1.6);
            }
            Button::MetalMetal => self.metal_metal = !self.metal_metal,
            Button::Smooth => self.smooth = !self.smooth,
            Button::RelativeEnergy => self.relative_energy = !self.relative_energy,
            Button::Light => self.light = !self.light,
            Button::LessIso => self.iso = (self.iso / ISO_STEP).max(MIN_ISO),
            Button::MoreIso => self.iso = (self.iso * ISO_STEP).min(MAX_ISO),
            Button::Forces => self.forces = !self.forces,
            Button::Transparent => {
                self.transparent = !self.transparent;
                self.smooth |= self.transparent;
            }
            Button::FixOrientation => {
                self.fixed_orientation = match self.fixed_orientation {
                    Some(_) => None,
                    None => {
                        let (all, cur) = (&self.loaded.structures, self.player.current);
                        Some(
                            (0..all.len())
                                .map(|i| match i == cur {
                                    true => IDENTITY,
                                    false => orientation_to(&all[i], &all[cur]),
                                })
                                .collect(),
                        )
                    }
                }
            }
        }
    }

    pub fn handle(&mut self, input: Input, now: f64) -> bool {
        let before = (self.player.current, self.player.playing, self.hover);
        match input {
            Input::Resize { w, h } => {
                let (cw, lh) = (self.layout.char_w, self.layout.line_h);
                self.layout = Layout::with_panes(w, h, cw, lh, self.layout.panes);
                return true;
            }
            Input::Key(k) => match k {
                Key::Right => self.player.next(),
                Key::Left => self.player.prev(),
                Key::Home => self.player.go(0),
                Key::End => self.player.go(usize::MAX),
                Key::Space => self.player.toggle(now),
                Key::Quit => self.quit = true,
            },
            Input::Press { x, y, button } => {
                self.pointer = (x, y);
                let l = &self.layout;
                if button == 1
                    && let Some(b) = l.button_at(x, y)
                {
                    self.press(b, now);
                    return true;
                }
                if button == 1
                    && let Some(edge) = l.edge_at(x, y)
                {
                    self.resizing = Some(edge);
                    return true;
                }
                if l.plot.contains(x, y) {
                    if button == 1 {
                        self.scrub = true;
                        self.scrub_to(x);
                    }
                } else if l.view.contains(x, y) {
                    match button {
                        1..=3 => {
                            let double = button == 1
                                && self.last_click.is_some_and(|(t, px, py)| {
                                    now - t < 0.35 && (px - x).abs() + (py - y).abs() < 6
                                });
                            if double {
                                (self.rotation, self.zoom, self.pan) = (IDENTITY, 1.0, (0.0, 0.0));
                                self.last_click = None;
                                return true;
                            }
                            if button == 1 {
                                self.last_click = Some((now, x, y));
                            }
                            self.drag = Some((button, x, y));
                        }
                        4 | 5 => {
                            let f = if button == 4 { 1.1 } else { 1.0 / 1.1 };
                            self.zoom = (self.zoom * f).clamp(0.05, 50.0);
                            return true;
                        }
                        _ => {}
                    }
                }
            }
            Input::Release { button } => {
                self.scrub &= button != 1;
                if button == 1 {
                    self.resizing = None;
                }
                if self.drag.is_some_and(|(b, _, _)| b == button) {
                    self.drag = None;
                }
            }
            Input::Motion { x, y } => {
                self.pointer = (x, y);
                if let Some(edge) = self.resizing {
                    let l = &self.layout;
                    let mut panes = l.panes;
                    match edge {
                        Edge::Plot => panes.plot_h = Some(l.h - y),
                        Edge::Panel => panes.panel_w = Some(l.w - x),
                    }
                    self.layout = Layout::with_panes(l.w, l.h, l.char_w, l.line_h, panes);
                    self.hover = Hover::Edge(edge);
                    return true;
                }
                if let Some((b, px, py)) = self.drag {
                    let (dx, dy) = (f64::from(x - px), f64::from(y - py));
                    if b == 1 {
                        self.rotation = mul(&rotation(dy * 0.01, dx * 0.01), &self.rotation);
                    } else {
                        self.pan = (self.pan.0 + dx, self.pan.1 + dy);
                    }
                    self.drag = Some((b, x, y));
                    self.hover = Hover::None;
                    return true;
                }
                if self.scrub {
                    self.scrub_to(x);
                }
            }
            Input::Leave => {
                self.hover = Hover::None;
                return before.2 != Hover::None;
            }
        }
        if self.drag.is_none() {
            self.hover = self.hover_at(self.pointer.0, self.pointer.1);
        }
        before != (self.player.current, self.player.playing, self.hover)
            || matches!(input, Input::Key(_) | Input::Press { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mole_core::Atom;
    use std::path::PathBuf;

    fn s(energy: Option<f64>) -> Structure {
        Structure {
            atoms: vec![
                Atom {
                    z: 8,
                    pos: [0.0, 0.0, 0.0],
                },
                Atom {
                    z: 1,
                    pos: [0.96, 0.0, 0.0],
                },
            ],
            energy,
            energy_source: None,
            label: String::new(),
            comment: String::new(),
            path: PathBuf::new(),
            line: 1,
            columns: Vec::new(),
            grid: None,
        }
    }

    fn app(n: usize) -> App {
        App::new(
            Loaded {
                structures: (0..n).map(|i| s(Some(i as f64))).collect(),
                ..Default::default()
            },
            Layout::new(1200, 800, 9, 15),
        )
    }

    fn click(app: &mut App, x: i32, y: i32) {
        app.handle(Input::Motion { x, y }, 0.0);
        app.handle(Input::Press { x, y, button: 1 }, 0.0);
        app.handle(Input::Release { button: 1 }, 0.0);
    }

    fn center(app: &App, item: BarItem) -> (i32, i32) {
        let r = app.layout.items.iter().find(|i| i.0 == item).unwrap().1;
        (r.x + r.w / 2, r.y + r.h / 2)
    }

    #[test]
    fn navigation_and_looping() {
        let mut p = Player::new(3);
        p.prev();
        assert_eq!(p.current, 2, "anterior desde la primera da la vuelta");
        p.next();
        assert_eq!(p.current, 0);
        p.looping = false;
        p.go(2);
        p.playing = true;
        p.next();
        assert_eq!(
            (p.current, p.playing),
            (2, false),
            "sin repetición se detiene al final"
        );
        p.toggle(0.0);
        assert_eq!(
            (p.current, p.playing),
            (0, true),
            "reproducir desde el final reinicia"
        );
        p.go(usize::MAX);
        assert_eq!(p.current, 2);
    }

    #[test]
    fn playback_timing_and_speed() {
        let mut p = Player::new(4);
        assert_eq!(p.tick(10.0), None, "en pausa no avanza");
        p.speed = 2.0;
        p.toggle(0.0);
        assert!(p.tick(0.2).is_some());
        assert_eq!(p.current, 0);
        p.tick(0.5);
        assert_eq!(p.current, 1);
        p.tick(1.0);
        assert_eq!(p.current, 2);
        assert_eq!(Player::new(1).tick(5.0), None);
        p.change_speed(true);
        assert_eq!(p.speed, 4.0);
        p.speed = 120.0;
        p.change_speed(true);
        assert_eq!(p.speed, 120.0);
        p.speed = 0.5;
        p.change_speed(false);
        assert_eq!(p.speed, 0.5);
    }

    #[test]
    fn nearest_structure_with_energy() {
        let st = [s(Some(1.0)), s(None), s(Some(3.0)), s(Some(0.5))];
        assert_eq!(nearest_with_energy(&st, 1.2), Some(0));
        assert_eq!(
            nearest_with_energy(&st, 2.4),
            Some(2),
            "salta la que no tiene energía"
        );
        assert_eq!(nearest_with_energy(&st, 9.0), Some(3));
        assert_eq!(nearest_with_energy(&[s(None)], 1.0), None);
    }

    #[test]
    fn layout_fits_and_buttons_do_not_overlap() {
        let l = Layout::new(1200, 800, 9, 15);
        assert_eq!(l.header.h + l.view.h + l.plot.h, 800);
        assert_eq!(l.view.w + l.panel.w, 1200);
        assert!(l.panel.w < 240, "columna estrecha: {}", l.panel.w);
        for b in [
            Button::First,
            Button::Prev,
            Button::Play,
            Button::Next,
            Button::Last,
        ] {
            let r = l
                .items
                .iter()
                .find(|i| i.0 == BarItem::Button(b))
                .unwrap()
                .1;
            assert!(r.h >= 34 && r.w > l.panel.w - 20, "{b:?} pequeño: {r:?}");
        }
        let bottom = l.items.iter().map(|i| i.1.y + i.1.h).max().unwrap();
        assert!(bottom < 800 * 4 / 5, "los controles bajan hasta {bottom}");
        let inside = |a: Rect, b: Rect| {
            b.x <= a.x && a.x + a.w <= b.x + b.w && b.y <= a.y && a.y + a.h <= b.y + b.h
        };
        let rects: Vec<Rect> = l.items.iter().map(|i| i.1).collect();
        for (i, a) in rects.iter().enumerate() {
            assert!(
                inside(*a, l.panel),
                "{:?} se sale de la columna",
                l.items[i]
            );
            for b in &rects[i + 1..] {
                let apart =
                    a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y;
                assert!(apart, "{a:?} y {b:?} se solapan");
            }
        }
        for (item, text) in [
            (BarItem::Speed, speed_text(120.0)),
            (BarItem::Tolerance, tolerance_text(1.6)),
        ] {
            let r = l.items.iter().find(|i| i.0 == item).unwrap().1;
            assert!(9 * text.len() as i32 <= r.w, "{text} no cabe en {r:?}");
        }
    }

    #[test]
    fn playback_buttons() {
        let mut a = app(5);
        let press = |a: &mut App, b| {
            let (x, y) = center(a, BarItem::Button(b));
            click(a, x, y);
        };
        press(&mut a, Button::Last);
        assert_eq!(a.player.current, 4);
        press(&mut a, Button::Prev);
        assert_eq!(a.player.current, 3);
        press(&mut a, Button::First);
        assert_eq!(a.player.current, 0);
        press(&mut a, Button::Next);
        assert_eq!(a.player.current, 1);
        press(&mut a, Button::Play);
        assert!(a.player.playing);
        press(&mut a, Button::Faster);
        assert_eq!(a.player.speed, 8.0);
        press(&mut a, Button::Loop);
        assert!(!a.player.looping);
        press(&mut a, Button::MoreTolerance);
        assert!((a.bond_tolerance - 1.30).abs() < 1e-9);
        assert!(a.metal_metal, "enlaces metal–metal activados por defecto");
        press(&mut a, Button::MetalMetal);
        assert!(!a.metal_metal);
        assert!(a.relative_energy, "eje E relativo por defecto");
        assert_eq!(a.plot_map().unwrap().e_ref, Some(0.0));
        press(&mut a, Button::RelativeEnergy);
        assert!(!a.relative_energy);
        assert_eq!(a.plot_map().unwrap().e_ref, None);
        press(&mut a, Button::Smooth);
        assert!(a.smooth);
    }

    #[test]
    fn keyboard_shortcuts() {
        let mut a = app(5);
        let mut key = |k| {
            a.handle(Input::Key(k), 0.0);
            a.player.current
        };
        assert_eq!(key(Key::Right), 1);
        assert_eq!(key(Key::End), 4);
        assert_eq!(key(Key::Left), 3);
        assert_eq!(key(Key::Home), 0);
        a.handle(Input::Key(Key::Space), 0.0);
        assert!(a.player.playing);
        a.handle(Input::Key(Key::Quit), 0.0);
        assert!(a.quit);
    }

    #[test]
    fn plot_height_follows_the_dragged_edge() {
        let mut a = app(5);
        let (h0, y0) = (a.layout.plot.h, a.layout.plot.y);
        a.handle(Input::Motion { x: 100, y: y0 }, 0.0);
        assert_eq!(a.hover, Hover::Edge(Edge::Plot));
        a.handle(
            Input::Press {
                x: 100,
                y: y0,
                button: 1,
            },
            0.0,
        );
        a.handle(
            Input::Motion {
                x: 100,
                y: y0 - 100,
            },
            0.0,
        );
        a.handle(Input::Release { button: 1 }, 0.0);
        let l = &a.layout;
        assert_eq!(l.plot.h, h0 + 100);
        assert_eq!(l.header.h + l.view.h + l.plot.h, l.h);
        a.handle(
            Input::Press {
                x: 100,
                y: a.layout.plot.y,
                button: 1,
            },
            0.0,
        );
        a.handle(Input::Motion { x: 100, y: 5000 }, 0.0);
        a.handle(Input::Release { button: 1 }, 0.0);
        assert_eq!(a.layout.plot.h, MIN_PANE_H);
        a.handle(Input::Resize { w: 1400, h: 900 }, 0.0);
        assert_eq!(a.layout.plot.h, MIN_PANE_H, "se conserva al redimensionar");
        assert_eq!(a.player.current, 0);
    }

    fn mol(atoms: &[(u8, [f64; 3])]) -> Structure {
        Structure {
            atoms: atoms.iter().map(|&(z, pos)| Atom { z, pos }).collect(),
            ..s(None)
        }
    }

    const ASYM: [(u8, [f64; 3]); 5] = [
        (6, [0.0, 0.0, 0.0]),
        (8, [1.2, 0.1, 0.0]),
        (1, [-0.6, 0.9, 0.2]),
        (7, [-0.4, -1.0, 0.5]),
        (17, [0.3, 0.2, -1.7]),
    ];

    fn turn(atoms: &[(u8, [f64; 3])]) -> Vec<(u8, [f64; 3])> {
        let r = mul(&rotation(0.7, -1.2), &rotation(2.1, 0.4));
        atoms
            .iter()
            .map(|&(z, p)| (z, apply(&r, p).map(|v| v + 3.0)))
            .collect()
    }

    fn close(a: [f64; 3], b: [f64; 3]) -> bool {
        (0..3).all(|k| (a[k] - b[k]).abs() < 1e-6)
    }

    #[test]
    fn fixed_orientation_superposes_same_atoms() {
        let (a, b) = (mol(&ASYM), mol(&turn(&ASYM)));
        let m = orientation_to(&b, &a);
        let rotated: Vec<_> = centered(&b).iter().map(|&p| apply(&m, p)).collect();
        assert!(rotated.iter().zip(centered(&a)).all(|(&p, q)| close(p, q)));
        let det = dot(m[0], cross(m[1], m[2]));
        assert!((det - 1.0).abs() < 1e-9, "rotación propia, sin reflejar");

        let mut app = App::new(
            Loaded {
                structures: vec![a, b],
                ..Default::default()
            },
            Layout::new(1200, 800, 9, 15),
        );
        app.press(Button::FixOrientation, 0.0);
        let reference = app.projection().0;
        app.player.go(1);
        assert!(
            app.projection()
                .0
                .iter()
                .zip(&reference)
                .all(|(&p, &q)| close(p, q))
        );
        app.press(Button::FixOrientation, 0.0);
        assert!(
            app.fixed_orientation.is_none(),
            "otra pulsación vuelve a las del archivo"
        );
    }

    #[test]
    fn fixed_orientation_aligns_isomers_by_principal_axes() {
        let mut shuffled = turn(&ASYM);
        shuffled.rotate_left(2);
        let (a, b) = (mol(&ASYM), mol(&shuffled));
        let m = orientation_to(&b, &a);
        let target = centered(&a);
        for (atom, p) in b.atoms.iter().zip(centered(&b)) {
            let p = apply(&m, p);
            let found = a
                .atoms
                .iter()
                .zip(&target)
                .any(|(t, &q)| t.z == atom.z && close(p, q));
            assert!(found, "Z = {} en {p:?} no coincide", atom.z);
        }
    }

    fn cube_app() -> App {
        let n = 20;
        let at = |v: usize| (v as f64 - 9.5) * 0.5;
        let values = (0..n * n * n)
            .map(|i| {
                let (x, y, z) = (at(i / (n * n)), at(i / n % n), at(i % n));
                (z * (-(x * x + y * y + z * z) / 4.0).exp()) as f32
            })
            .collect();
        let grid = mole_core::cube::Grid {
            n: [n; 3],
            origin: [-4.75; 3],
            axes: [[0.5, 0.0, 0.0], [0.0, 0.5, 0.0], [0.0, 0.0, 0.5]],
            values,
        };
        App::new(
            Loaded {
                structures: vec![Structure {
                    columns: Vec::new(),
                    grid: Some(Box::new(grid)),
                    ..s(None)
                }],
                ..Default::default()
            },
            Layout::new(1200, 800, 9, 15),
        )
    }

    #[test]
    fn cube_shows_two_lobes_and_its_controls() {
        let mut a = cube_app();
        assert_eq!(a.layout.plot.h, MIN_PANE_H, "sin energías, gráfica mínima");
        let faces = |a: &mut App| {
            let mut colors = [0, 0];
            for shape in a.scene() {
                if let Shape3d::Quad { rgb, .. } = shape {
                    colors[usize::from(rgb[2] > rgb[0])] += 1;
                }
            }
            colors
        };
        let [red, blue] = faces(&mut a);
        assert!(red > 50 && blue > 50, "{red} rojas, {blue} azules");
        let iso = a.iso;
        a.press(Button::MoreIso, 0.0);
        assert!(a.iso > iso);
        let [smaller, _] = faces(&mut a);
        assert!(
            smaller < red,
            "más iso, lóbulo más pequeño: {smaller} frente a {red}"
        );
        a.press(Button::Transparent, 0.0);
        assert!(
            a.transparent && a.smooth,
            "la transparencia necesita XRender"
        );
        assert!(a.layout.items.iter().any(|i| i.0 == BarItem::Iso));
        let bottom = a.layout.items.iter().map(|i| i.1.y + i.1.h).max().unwrap();
        assert!(bottom < a.layout.h, "los controles caben: {bottom}");
        assert!(!app(2).layout.items.iter().any(|i| i.0 == BarItem::Iso));
    }

    #[test]
    fn command_line_options() {
        let mut a = app(5);
        let o = crate::cli::Options {
            absolute: true,
            no_mm: true,
            transparent: true,
            frame: Some(3),
            tol: Some(1.4),
            iso: Some(0.05),
            ..Default::default()
        };
        a.configure(&o);
        assert_eq!(a.player.current, 2);
        assert!(!a.relative_energy && !a.metal_metal);
        assert!(
            a.transparent && a.smooth,
            "la transparencia necesita XRender"
        );
        assert_eq!((a.bond_tolerance, a.iso), (1.4, 0.05));
        let mut a = app(2);
        a.configure(&crate::cli::Options {
            frame: Some(99),
            ..Default::default()
        });
        assert_eq!(a.player.current, 1, "--frame mayor que el total: la última");
    }

    #[test]
    fn force_arrows_and_column_values() {
        let mut st = s(Some(0.0));
        st.columns = vec![
            mole_core::Column {
                name: "q".into(),
                width: 1,
                values: vec![-0.5, f64::NAN],
            },
            mole_core::Column {
                name: "forces".into(),
                width: 3,
                values: vec![1e-5, 0.0, 0.0, 0.0, 2e-5, 0.0],
            },
        ];
        let loaded = Loaded {
            structures: vec![st],
            ..Default::default()
        };
        let mut a = App::new(loaded, Layout::new(1200, 800, 9, 15));
        let arrows = |a: &mut App| {
            a.scene()
                .iter()
                .filter_map(|s| match *s {
                    Shape3d::Arrow { from, head, .. } => {
                        Some((head[0].0 - from.0).hypot(head[0].1 - from.1))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let len = arrows(&mut a);
        assert_eq!(len.len(), 2, "una flecha por átomo");
        assert!((len[1] - 2.0 * len[0]).abs() < 1e-6, "{len:?}");
        assert!(
            a.layout
                .items
                .iter()
                .any(|i| i.0 == BarItem::Button(Button::Forces))
        );
        a.press(Button::Forces, 0.0);
        assert!(arrows(&mut a).is_empty());
        assert!(
            !app(1)
                .layout
                .items
                .iter()
                .any(|i| i.0 == BarItem::Button(Button::Forces))
        );
        let st = a.structure();
        assert_eq!(
            column_lines(st, 0),
            ["q=-0.5  forces=(1.00e-5, 0, 0) |F|=1.00e-5"]
        );
        assert_eq!(
            column_lines(st, 1)[0],
            "q=NaN  forces=(0, 2.00e-5, 0) |F|=2.00e-5"
        );
        assert!(column_lines(st, 5).is_empty());
    }

    #[test]
    fn surface_has_a_face_limit() {
        let mut a = cube_app();
        a.update_surface();
        assert!(a.surface.1.len() <= MAX_QUADS);
        let grid = a.loaded.structures[0].grid.as_ref().unwrap();
        assert!(
            grid.isosurface(a.iso, 1).len() + grid.isosurface(-a.iso, 1).len() >= a.surface.1.len()
        );
    }

    #[test]
    fn panel_width_follows_its_edge_and_never_hides_controls() {
        let mut a = app(3);
        let (w0, x) = (a.layout.panel.w, a.layout.panel.x);
        a.handle(Input::Motion { x, y: 300 }, 0.0);
        assert_eq!(a.hover, Hover::Edge(Edge::Panel));
        a.handle(
            Input::Press {
                x,
                y: 300,
                button: 1,
            },
            0.0,
        );
        a.handle(Input::Motion { x: x - 80, y: 300 }, 0.0);
        a.handle(Input::Release { button: 1 }, 0.0);
        assert_eq!(a.layout.panel.w, w0 + 80);
        assert_eq!(a.layout.view.w + a.layout.panel.w, a.layout.w);
        a.handle(
            Input::Press {
                x: a.layout.panel.x,
                y: 300,
                button: 1,
            },
            0.0,
        );
        a.handle(Input::Motion { x: 5000, y: 300 }, 0.0);
        a.handle(Input::Release { button: 1 }, 0.0);
        assert_eq!(
            a.layout.panel.w, w0,
            "no baja de lo que piden los controles"
        );
    }

    #[test]
    fn static_plot_click_hover_and_scrub() {
        let mut a = app(10);
        let m = a.plot_map().unwrap();
        let (x, y) = (m.x(5.0) as i32, m.y(4.0) as i32);
        a.handle(Input::Motion { x, y }, 0.0);
        assert_eq!(a.hover, Hover::Plot(4), "pasar el ratón muestra n = 5");
        click(&mut a, x, y);
        assert_eq!(a.player.current, 4);
        a.handle(Input::Press { x, y, button: 1 }, 0.0);
        a.handle(
            Input::Motion {
                x: m.x(8.0) as i32,
                y,
            },
            0.0,
        );
        assert_eq!(a.player.current, 7, "arrastrar recorre las estructuras");
        a.handle(Input::Motion { x: x + 5000, y }, 0.0);
        assert_eq!(a.player.current, 9, "fuera del área, la del extremo");
        a.handle(Input::Release { button: 1 }, 0.0);
        a.handle(Input::Motion { x, y }, 0.0);
        assert_eq!(a.player.current, 9, "al soltar deja de recorrer");
        assert_eq!(
            a.plot_map().unwrap().area,
            m.area,
            "la gráfica no se desplaza"
        );
    }

    #[test]
    fn view_rotation_hover_zoom_and_reset() {
        let mut a = app(1);
        let shapes = a.scene();
        assert_eq!(shapes.len(), 4, "2 esferas y 2 medios enlaces: {shapes:?}");
        let Shape3d::Sphere { x, y, .. } = shapes
            .iter()
            .find(|s| matches!(s, Shape3d::Sphere { rgb, .. } if rgb[0] > 0.8 && rgb[1] < 0.3))
            .copied()
            .unwrap()
        else {
            unreachable!()
        };
        a.handle(
            Input::Motion {
                x: x as i32,
                y: y as i32,
            },
            0.0,
        );
        assert_eq!(a.hover, Hover::Atom(0), "el oxígeno bajo el puntero");
        let before = a.scene();
        a.handle(
            Input::Press {
                x: 300,
                y: 300,
                button: 1,
            },
            1.0,
        );
        a.handle(Input::Motion { x: 330, y: 340 }, 1.0);
        a.handle(Input::Release { button: 1 }, 1.0);
        assert_ne!(a.scene(), before, "arrastrar rota");
        a.handle(
            Input::Press {
                x: 300,
                y: 300,
                button: 4,
            },
            2.0,
        );
        assert!(a.zoom > 1.0);
        a.handle(
            Input::Press {
                x: 300,
                y: 300,
                button: 1,
            },
            3.0,
        );
        a.handle(Input::Release { button: 1 }, 3.0);
        a.handle(
            Input::Press {
                x: 301,
                y: 300,
                button: 1,
            },
            3.2,
        );
        assert_eq!(a.scene(), before, "doble clic restablece");
    }

    #[test]
    fn ticks() {
        let (t, step) = nice_ticks(-3219850.0, -3219778.0, 6);
        assert_eq!(step, 20.0);
        assert_eq!(t.first(), Some(&-3219840.0));
        assert_eq!(tick_text(-3219840.0, step), "-3219840");
        let (t, step) = nice_ticks(-74.9659, -74.9624, 5);
        assert_eq!(step, 0.001);
        assert_eq!(tick_text(t[0], step), "-74.965");
        let (t, _) = nice_ticks(0.5, 4920.5, 8);
        assert_eq!(t, vec![1000.0, 2000.0, 3000.0, 4000.0]);
        let (t, step) = nice_ticks(0.0, 0.0000083, 5);
        let k = tick_exponent(&t, step);
        assert_eq!(k, -6);
        let f = 10f64.powi(k);
        assert_eq!(tick_text(t[1] / f, step / f), "2");
        let (t, step) = nice_ticks(-74.96501, -74.96499, 5);
        assert_eq!(tick_exponent(&t, step), 0);
        assert_eq!(tick_exponent(&[0.0, 0.002], 0.001), 0);
        assert_eq!(short_number(0.0000012345), "1.23e-6");
        assert_eq!(short_number(1.8200000000651926), "1.82");
        assert_eq!(short_number(0.0), "0");
    }

    #[test]
    fn rotations_are_orthonormal() {
        let m = mul(&rotation(0.7, -1.3), &IDENTITY);
        let v = apply(&m, [1.0, 2.0, 3.0]);
        let len = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        assert!((len(v) - len([1.0, 2.0, 3.0])).abs() < 1e-12);
    }
}
