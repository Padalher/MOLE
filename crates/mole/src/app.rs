//! Estado del visor, distribución de la ventana y respuesta al ratón y al teclado. Sin X11: todo lo
//! de aquí se prueba sin pantalla; `x11.rs` solo traduce eventos y dibuja.

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

/// Rotación de `ax` rad alrededor del eje x de la pantalla y `ay` alrededor del eje y.
fn rotation(ax: f64, ay: f64) -> Mat3 {
    let (sx, cx) = ax.sin_cos();
    let (sy, cy) = ay.sin_cos();
    mul(
        &[[1.0, 0.0, 0.0], [0.0, cx, -sx], [0.0, sx, cx]],
        &[[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]],
    )
}

/// Navegación y reproducción.
pub struct Player {
    pub len: usize,
    pub current: usize,
    pub playing: bool,
    pub looping: bool,
    /// Estructuras por segundo.
    pub speed: f64,
    last_step: f64,
}

/// Velocidades que recorren los botones − y +.
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
        // Reproducir desde el final sin repetición vuelve a empezar.
        if self.playing && !self.looping && self.current + 1 == self.len {
            self.current = 0;
        }
    }

    /// Avanza si toca; devuelve los segundos hasta el siguiente paso.
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

/// Índice de la estructura con energía más cercana a `x` (eje n, 1-based).
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
}

/// Elemento de la barra de controles: botón o texto (con su ancho fijo en caracteres).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BarItem {
    Button(Button),
    Speed,
    Tolerance,
}

/// Rectángulos de cada zona y de cada control; dependen solo del tamaño de la ventana y de la
/// fuente (monoespaciada), no del estado.
#[derive(Debug, Clone, Default)]
pub struct Layout {
    pub w: i32,
    pub h: i32,
    pub char_w: i32,
    pub line_h: i32,
    pub header: Rect,
    pub view: Rect,
    pub plot: Rect,
    pub bar: Rect,
    pub items: Vec<(BarItem, Rect)>,
}

/// Textos de ancho fijo de la barra (el valor cambia, el ancho no).
pub fn speed_text(speed: f64) -> String {
    format!("{speed:>5} structures/s")
}

pub fn tolerance_text(t: f64) -> String {
    format!("bond tolerance {t:.2}")
}

impl Layout {
    pub fn new(w: i32, h: i32, char_w: i32, line_h: i32) -> Self {
        let header = Rect {
            x: 0,
            y: 0,
            w,
            h: line_h + 10,
        };
        let bar_h = (line_h + 14).max(32);
        let bar = Rect {
            x: 0,
            y: h - bar_h,
            w,
            h: bar_h,
        };
        let plot_h = (h / 3).clamp(120, 280);
        let plot = Rect {
            x: 0,
            y: bar.y - plot_h,
            w,
            h: plot_h,
        };
        let view = Rect {
            x: 0,
            y: header.h,
            w,
            h: (plot.y - header.h).max(0),
        };
        let (bh, by) = (bar_h - 8, bar.y + 4);
        let mut x = 6;
        let mut items = Vec::new();
        let mut push = |item, width: i32, gap: i32| {
            items.push((
                item,
                Rect {
                    x,
                    y: by,
                    w: width,
                    h: bh,
                },
            ));
            x += width + gap;
        };
        let sq = bh + 6;
        for b in [Button::First, Button::Prev, Button::Play, Button::Next] {
            push(BarItem::Button(b), sq, 4);
        }
        push(BarItem::Button(Button::Last), sq, 16);
        let small = bh;
        push(BarItem::Button(Button::Slower), small, 4);
        push(BarItem::Speed, char_w * speed_text(0.0).len() as i32 + 4, 4);
        push(BarItem::Button(Button::Faster), small, 12);
        push(BarItem::Button(Button::Loop), small + char_w * 5, 20);
        push(BarItem::Button(Button::LessTolerance), small, 4);
        push(
            BarItem::Tolerance,
            char_w * tolerance_text(0.0).len() as i32 + 4,
            4,
        );
        push(BarItem::Button(Button::MoreTolerance), small, 12);
        push(
            BarItem::Button(Button::MetalMetal),
            small + char_w * "Metal-metal bonds".len() as i32 + 6,
            20,
        );
        push(
            BarItem::Button(Button::Smooth),
            small + char_w * "Smooth".len() as i32 + 6,
            0,
        );
        Layout {
            w,
            h,
            char_w,
            line_h,
            header,
            view,
            plot,
            bar,
            items,
        }
    }

    pub fn button_at(&self, x: i32, y: i32) -> Option<Button> {
        self.items.iter().find_map(|&(item, r)| match item {
            BarItem::Button(b) if r.contains(x, y) => Some(b),
            _ => None,
        })
    }
}

/// Transformación de la gráfica E vs n: área de dibujo y rangos (con margen).
#[derive(Debug, Clone, Copy)]
pub struct PlotMap {
    pub area: Rect,
    pub n: (f64, f64),
    pub e: (f64, f64),
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

/// Marcas "bonitas" (1, 2 o 5 × 10ᵏ) entre `lo` y `hi`, como mucho unas `max` marcas.
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

/// Texto de una marca con los decimales justos para el paso.
pub fn tick_text(v: f64, step: f64) -> String {
    let decimals = (-step.log10().floor()).max(0.0) as usize;
    format!("{v:.decimals$}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Hover {
    #[default]
    None,
    Atom(usize),
    Plot(usize),
    Button(Button),
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

/// Eventos de entrada, ya independientes de X11. Botones: 1 izquierdo, 2 central, 3 derecho,
/// 4/5 rueda.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Input {
    Press { x: i32, y: i32, button: u8 },
    Release { button: u8 },
    Motion { x: i32, y: i32 },
    Key(Key),
    Resize { w: i32, h: i32 },
    Leave,
}

/// Lo que se dibuja de la escena, ya proyectado a píxeles y en orden de pintor.
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
}

/// Color por elemento: paleta propia inspirada en la convención CPK (O rojo, N azul, C gris…).
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
        z if elements::is_metal(z) => [150, 150, 165],
        _ => [200, 120, 200],
    };
    c.map(|v| f64::from(v) / 255.0)
}

/// Radio de dibujo en Å (bolas y barras).
fn ball_radius(z: u8) -> f64 {
    0.2 + 0.3 * elements::covalent_radius(z)
}

const BOND_RADIUS: f64 = 0.12;

pub struct App {
    pub loaded: Loaded,
    pub player: Player,
    pub bond_tolerance: f64,
    pub metal_metal: bool,
    /// Dibujo suavizado (XRender) en lugar de primitivas X11 básicas.
    pub smooth: bool,
    pub layout: Layout,
    pub hover: Hover,
    pub pointer: (i32, i32),
    pub quit: bool,
    rotation: Mat3,
    zoom: f64,
    pan: (f64, f64),
    /// Radio de encuadre común (Å) para que el tamaño no salte al cambiar de estructura.
    extent: f64,
    /// (botón, última posición) del arrastre en curso en la vista 3D.
    drag: Option<(u8, i32, i32)>,
    last_click: Option<(f64, i32, i32)>,
    bonds: BondCache,
}

/// Enlaces de la estructura actual y la clave (estructura, tolerancia, metal–metal) con la que se
/// calcularon.
type BondCache = (Option<(usize, f64, bool)>, Vec<(usize, usize)>);

impl App {
    pub fn new(loaded: Loaded, layout: Layout) -> Self {
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
            metal_metal: false,
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

    /// Gráfica: `None` si ninguna estructura tiene energía.
    pub fn plot_map(&self) -> Option<PlotMap> {
        let mut es = self.loaded.structures.iter().filter_map(|s| s.energy);
        let first = es.next()?;
        let (lo, hi) = es.fold((first, first), |(lo, hi), e| (lo.min(e), hi.max(e)));
        let pad = ((hi - lo) * 0.06).max(lo.abs().max(1.0) * 1e-9);
        let (cw, lh) = (self.layout.char_w, self.layout.line_h);
        let p = self.layout.plot;
        let left = cw * 13;
        let area = Rect {
            x: p.x + left,
            y: p.y + lh / 2 + 6,
            w: (p.w - left - cw * 3).max(10),
            h: (p.h - 2 * lh - 14).max(10),
        };
        let n = self.loaded.structures.len() as f64;
        Some(PlotMap {
            area,
            n: (0.5, n + 0.5),
            e: (lo - pad, hi + pad),
        })
    }

    /// Posiciones proyectadas: (x, y) en píxeles, z de vista (Å, hacia el observador) y escala
    /// (píxeles por Å).
    fn projection(&self) -> (Vec<[f64; 3]>, f64) {
        let atoms = &self.structure().atoms;
        let v = self.layout.view;
        let n = atoms.len().max(1) as f64;
        let c = [0, 1, 2].map(|k| atoms.iter().map(|a| a.pos[k]).sum::<f64>() / n);
        let scale = self.zoom * f64::from(v.w.min(v.h)) * 0.45 / self.extent.max(1.0);
        let (ox, oy) = (
            f64::from(v.x) + f64::from(v.w) / 2.0 + self.pan.0,
            f64::from(v.y) + f64::from(v.h) / 2.0 + self.pan.1,
        );
        let pts = atoms
            .iter()
            .map(|a| {
                let p = apply(&self.rotation, [0, 1, 2].map(|k| a.pos[k] - c[k]));
                [ox + p[0] * scale, oy - p[1] * scale, p[2]]
            })
            .collect();
        (pts, scale)
    }

    /// Escena en orden de pintor (de atrás hacia delante).
    pub fn scene(&mut self) -> Vec<Shape3d> {
        let bonds = self.current_bonds().to_vec();
        let (p, scale) = self.projection();
        let atoms = &self.structure().atoms;
        let (zmin, zmax) = p.iter().fold((f64::MAX, f64::MIN), |(lo, hi), q| {
            (lo.min(q[2]), hi.max(q[2]))
        });
        // Oscurecimiento por profundidad: lo más lejano, al 70 %.
        let fade = |z: f64| 1.0 - 0.3 * (zmax - z) / (zmax - zmin).max(1e-9);
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
                // En píxeles (z también escalado) para medir dentro de la esfera.
                let d = [o[0] - s[0], o[1] - s[1], (o[2] - s[2]) * scale];
                let half = 0.5 * d.iter().map(|c| c * c).sum::<f64>().sqrt();
                // El trazo empieza donde el cilindro corta la esfera (lo tapa la propia esfera).
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
                // Profundidad media del tramo: detrás de su esfera si sale por la cara oculta.
                items.push((0.5 * (start[2] + mid[2]), shape));
            }
        }
        items.sort_by(|x, y| x.0.total_cmp(&y.0));
        items.into_iter().map(|(_, s)| s).collect()
    }

    /// Átomo bajo el puntero (el más cercano al observador).
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

    fn hover_at(&self, x: i32, y: i32) -> Hover {
        let l = &self.layout;
        if let Some(b) = l.button_at(x, y) {
            Hover::Button(b)
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
        }
    }

    /// Aplica un evento; devuelve si hay que redibujar.
    pub fn handle(&mut self, input: Input, now: f64) -> bool {
        let before = (self.player.current, self.player.playing, self.hover);
        match input {
            Input::Resize { w, h } => {
                let (cw, lh) = (self.layout.char_w, self.layout.line_h);
                self.layout = Layout::new(w, h, cw, lh);
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
                if l.plot.contains(x, y) {
                    // Gráfica estática: solo el clic izquierdo selecciona.
                    if button == 1
                        && let Hover::Plot(i) = self.hover_at(x, y)
                    {
                        self.player.go(i);
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
                if self.drag.is_some_and(|(b, _, _)| b == button) {
                    self.drag = None;
                }
            }
            Input::Motion { x, y } => {
                self.pointer = (x, y);
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
        assert_eq!(l.header.h + l.view.h + l.plot.h + l.bar.h, 800);
        let rects: Vec<Rect> = l.items.iter().map(|i| i.1).collect();
        for (i, a) in rects.iter().enumerate() {
            assert!(a.x + a.w <= 1200, "{:?} se sale", l.items[i]);
            for b in &rects[i + 1..] {
                assert!(a.x + a.w <= b.x, "{a:?} y {b:?} se solapan");
            }
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
        press(&mut a, Button::MetalMetal);
        assert!(a.metal_metal);
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

    /// Clic en la gráfica: selecciona la estructura bajo el puntero; arrastrar no la desplaza.
    #[test]
    fn static_plot_click_and_hover() {
        let mut a = app(10);
        let m = a.plot_map().unwrap();
        let (x, y) = (m.x(5.0) as i32, m.y(4.0) as i32);
        a.handle(Input::Motion { x, y }, 0.0);
        assert_eq!(a.hover, Hover::Plot(4), "pasar el ratón muestra n = 5");
        // Arrastre en la gráfica: no hay desplazamiento posible.
        a.handle(Input::Press { x, y, button: 1 }, 0.0);
        a.handle(Input::Motion { x: x + 150, y }, 0.0);
        a.handle(Input::Release { button: 1 }, 0.0);
        click(&mut a, x, y);
        assert_eq!(a.player.current, 4);
        assert_eq!(a.plot_map().unwrap().area, m.area);
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
    }

    #[test]
    fn rotations_are_orthonormal() {
        let m = mul(&rotation(0.7, -1.3), &IDENTITY);
        let v = apply(&m, [1.0, 2.0, 3.0]);
        let len = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        assert!((len(v) - len([1.0, 2.0, 3.0])).abs() < 1e-12);
    }
}
