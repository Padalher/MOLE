//! Ventana y dibujo con el protocolo X11 básico (x11rb, sin bibliotecas de C). Por `ssh -X/-Y`
//! solo viajan órdenes de dibujo (unos KB por fotograma) que dibuja el servidor X del usuario; la
//! gráfica se dibuja una vez en un pixmap del servidor y se copia en cada fotograma.

use crate::app::{
    App, BarItem, Button, Hover, Input, Key, Layout, Rect, Shape3d, nice_ticks, speed_text,
    tick_text, tolerance_text,
};
use crate::cli::Options;
use mole_core::{Loaded, elements};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::error::Error;
use std::time::Instant;
use x11rb::connection::{Connection, RequestConnection as _};
use x11rb::protocol::Event;
use x11rb::protocol::render::{
    self, ConnectionExt as _, CreatePictureAux, Fixed, PictOp, PictType, Pictformat, Picture,
    Pointfix, PolyEdge, PolyMode,
};
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

type Res<T> = Result<T, Box<dyn Error>>;
type Rgb = [f64; 3];

const BG_VIEW: Rgb = [0.04, 0.04, 0.045];
const BG_PANEL: Rgb = [0.11, 0.11, 0.12];
const BG_PLOT: Rgb = [0.07, 0.07, 0.08];
const TEXT: Rgb = [0.9, 0.9, 0.92];
const WEAK: Rgb = [0.55, 0.55, 0.6];
const GRID: Rgb = [0.18, 0.18, 0.2];
const ACCENT: Rgb = [0.2, 0.6, 0.95];
const WARN: Rgb = [1.0, 0.65, 0.2];
const LINE: Rgb = [0.85, 0.35, 0.3];
const POINT: Rgb = [0.45, 0.6, 0.95];

/// Texto en ISO-8859-1 (fuentes del servidor); lo que no cabe se sustituye por `?`.
fn latin1(s: &str) -> Vec<u8> {
    s.chars()
        .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
        .collect()
}

/// Peticiones de dibujo con el contexto gráfico compartido; solo cambia color o grosor si hace
/// falta (cada cambio es una petición que viaja por la red).
struct Canvas<'a> {
    conn: &'a RustConnection,
    gc: Gcontext,
    masks: [u32; 3],
    fg: Cell<Option<u32>>,
    width: Cell<u32>,
    font: Cell<Font>,
}

impl Canvas<'_> {
    fn pixel(&self, rgb: Rgb) -> u32 {
        let mut p = 0;
        for (m, v) in self.masks.iter().zip(rgb) {
            let shift = m.trailing_zeros();
            let max = m >> shift;
            p |= (((v.clamp(0.0, 1.0) * f64::from(max)).round() as u32) << shift) & m;
        }
        p
    }

    fn set(&self, rgb: Rgb, width: u32) -> Res<()> {
        let px = self.pixel(rgb);
        let mut aux = ChangeGCAux::new();
        let mut changed = false;
        if self.fg.get() != Some(px) {
            aux = aux.foreground(px);
            self.fg.set(Some(px));
            changed = true;
        }
        if self.width.get() != width {
            aux = aux.line_width(width);
            self.width.set(width);
            changed = true;
        }
        if changed {
            self.conn.change_gc(self.gc, &aux)?;
        }
        Ok(())
    }

    fn set_font(&self, font: Font) -> Res<()> {
        if self.font.get() != font {
            self.conn
                .change_gc(self.gc, &ChangeGCAux::new().font(font))?;
            self.font.set(font);
        }
        Ok(())
    }

    fn rect(&self, d: Drawable, r: Rect, rgb: Rgb) -> Res<()> {
        self.set(rgb, self.width.get())?;
        self.conn.poly_fill_rectangle(
            d,
            self.gc,
            &[Rectangle {
                x: r.x as i16,
                y: r.y as i16,
                width: r.w.max(0) as u16,
                height: r.h.max(0) as u16,
            }],
        )?;
        Ok(())
    }

    fn outline(&self, d: Drawable, r: Rect, rgb: Rgb) -> Res<()> {
        self.set(rgb, 1)?;
        self.conn.poly_rectangle(
            d,
            self.gc,
            &[Rectangle {
                x: r.x as i16,
                y: r.y as i16,
                width: (r.w - 1).max(0) as u16,
                height: (r.h - 1).max(0) as u16,
            }],
        )?;
        Ok(())
    }

    fn circles(&self, d: Drawable, centers: &[(f64, f64)], r: f64, rgb: Rgb) -> Res<()> {
        self.set(rgb, self.width.get())?;
        let size = (2.0 * r).round().max(1.0) as u16;
        let arcs: Vec<Arc> = centers
            .iter()
            .map(|&(x, y)| Arc {
                x: (x - r).round() as i16,
                y: (y - r).round() as i16,
                width: size,
                height: size,
                angle1: 0,
                angle2: 360 * 64,
            })
            .collect();
        for chunk in arcs.chunks(8000) {
            self.conn.poly_fill_arc(d, self.gc, chunk)?;
        }
        Ok(())
    }

    fn segments(&self, d: Drawable, segs: &[Segment], width: u32, rgb: Rgb) -> Res<()> {
        self.set(rgb, width)?;
        for chunk in segs.chunks(8000) {
            self.conn.poly_segment(d, self.gc, chunk)?;
        }
        Ok(())
    }

    fn line(&self, d: Drawable, a: (f64, f64), b: (f64, f64), width: u32, rgb: Rgb) -> Res<()> {
        let s = Segment {
            x1: a.0.round() as i16,
            y1: a.1.round() as i16,
            x2: b.0.round() as i16,
            y2: b.1.round() as i16,
        };
        self.segments(d, &[s], width, rgb)
    }

    fn polygon(&self, d: Drawable, pts: &[(i32, i32)], rgb: Rgb) -> Res<()> {
        self.set(rgb, self.width.get())?;
        let pts: Vec<Point> = pts
            .iter()
            .map(|&(x, y)| Point {
                x: x as i16,
                y: y as i16,
            })
            .collect();
        self.conn
            .fill_poly(d, self.gc, PolyShape::CONVEX, CoordMode::ORIGIN, &pts)?;
        Ok(())
    }

    /// Texto con la línea base en `y`, sin fondo.
    fn text(&self, d: Drawable, x: i32, y: i32, s: &str, rgb: Rgb) -> Res<()> {
        self.set(rgb, self.width.get())?;
        let bytes = latin1(s);
        let mut items = Vec::with_capacity(bytes.len() + 2);
        for chunk in bytes.chunks(254) {
            items.push(chunk.len() as u8);
            items.push(0);
            items.extend_from_slice(chunk);
        }
        if !items.is_empty() {
            self.conn
                .poly_text8(d, self.gc, x as i16, y as i16, &items)?;
        }
        Ok(())
    }
}

/// Fuente monoespaciada del servidor: id, ancho de carácter, ascenso y alto de línea.
#[derive(Clone, Copy)]
struct FontInfo {
    id: Font,
    char_w: i32,
    ascent: i32,
    line_h: i32,
}

/// Abre la primera fuente disponible de `names` (las fuentes "misc fixed" vienen con casi todo
/// servidor X: Xorg, Xwayland, XQuartz, VcXsrv).
fn open_font(conn: &RustConnection, names: &[&str]) -> Res<FontInfo> {
    for name in names {
        let id = conn.generate_id()?;
        if conn.open_font(id, name.as_bytes())?.check().is_err() {
            continue;
        }
        let q = conn.query_font(id)?.reply()?;
        return Ok(FontInfo {
            id,
            char_w: i32::from(q.max_bounds.character_width).max(1),
            ascent: i32::from(q.font_ascent),
            line_h: i32::from(q.font_ascent) + i32::from(q.font_descent),
        });
    }
    Err(format!("ninguna fuente disponible en el servidor X: {names:?}").into())
}

/// Extensión XRender: imagen del pixmap de fondo, formatos ARGB32 y de máscara A8, y caché de
/// esferas ya dibujadas en el servidor.
struct Render {
    back: Picture,
    format: Pictformat,
    argb32: Pictformat,
    a8: Pictformat,
    /// (color, diámetro en px, nivel de profundidad) → (pixmap, imagen, diámetro).
    sprites: RefCell<SpriteCache>,
    /// Ídem para las texturas de cilindro de los enlaces (diámetro = grosor).
    sticks: RefCell<SpriteCache>,
}

/// (color RGB, tamaño en px, nivel de profundidad) → (pixmap, imagen, tamaño).
type SpriteCache = HashMap<(u32, u16, u8), (Pixmap, Picture, u16)>;

/// Largo (px) de la textura de cilindro; se estira a lo largo de cada enlace.
const STICK_LEN: u16 = 16;

/// Máximo de esferas en caché; al superarlo se vacía (p. ej. tras mucho zoom).
const MAX_SPRITES: usize = 512;

/// Prepara XRender si el servidor lo tiene (versión ≥ 0.10, con degradados).
fn init_render(conn: &RustConnection, screen_num: usize, visual: Visualid) -> Res<Option<Render>> {
    if conn
        .extension_information(render::X11_EXTENSION_NAME)?
        .is_none()
    {
        return Ok(None);
    }
    let v = conn.render_query_version(0, 11)?.reply()?;
    if (v.major_version, v.minor_version) < (0, 10) {
        return Ok(None);
    }
    let f = conn.render_query_pict_formats()?.reply()?;
    let format = f.screens.get(screen_num).and_then(|s| {
        s.depths
            .iter()
            .flat_map(|d| &d.visuals)
            .find(|pv| pv.visual == visual)
            .map(|pv| pv.format)
    });
    let find = |depth: u8, red: u16, shift: u16| {
        f.formats.iter().find(|i| {
            i.type_ == PictType::DIRECT
                && i.depth == depth
                && i.direct.alpha_mask == 0xff
                && i.direct.red_mask == red
                && (red == 0 || i.direct.red_shift == shift)
        })
    };
    let (a8, argb32) = (find(8, 0, 0), find(32, 0xff, 16));
    Ok(match (format, a8, argb32) {
        (Some(format), Some(a8), Some(argb32)) => Some(Render {
            back: 0,
            format,
            argb32: argb32.id,
            a8: a8.id,
            sprites: RefCell::new(HashMap::new()),
            sticks: RefCell::new(HashMap::new()),
        }),
        _ => None,
    })
}

fn fx(v: f64) -> Fixed {
    (v * 65536.0).round() as Fixed
}

fn pfx((x, y): (f64, f64)) -> Pointfix {
    Pointfix { x: fx(x), y: fx(y) }
}

fn rcolor(rgb: Rgb) -> render::Color {
    let c = |v: f64| (v.clamp(0.0, 1.0) * 65535.0).round() as u16;
    render::Color {
        red: c(rgb[0]),
        green: c(rgb[1]),
        blue: c(rgb[2]),
        alpha: 0xffff,
    }
}

struct Window<'a> {
    conn: &'a RustConnection,
    win: u32,
    back: Pixmap,
    plot: Pixmap,
    depth: u8,
    canvas: Canvas<'a>,
    font: FontInfo,
    bold: FontInfo,
    render: Option<Render>,
}

impl Window<'_> {
    fn make_pixmaps(&self, l: &Layout) -> Res<()> {
        let (w, h) = (l.w.max(1) as u16, l.h.max(1) as u16);
        self.conn
            .create_pixmap(self.depth, self.back, self.win, w, h)?;
        let ph = l.plot.h.max(1) as u16;
        self.conn
            .create_pixmap(self.depth, self.plot, self.win, w, ph)?;
        if let Some(r) = &self.render {
            let aux = CreatePictureAux::new()
                .polyedge(PolyEdge::SMOOTH)
                .polymode(PolyMode::IMPRECISE);
            self.conn
                .render_create_picture(r.back, self.back, r.format, &aux)?;
        }
        Ok(())
    }

    fn free_pixmaps(&self) -> Res<()> {
        if let Some(r) = &self.render {
            self.conn.render_free_picture(r.back)?;
        }
        self.conn.free_pixmap(self.back)?;
        self.conn.free_pixmap(self.plot)?;
        Ok(())
    }

    /// Esfera suavizada: se dibuja una vez por (color, tamaño, profundidad) en una imagen ARGB del
    /// servidor y en cada fotograma solo se copia (una petición de 36 bytes).
    fn sphere_aa(&self, r: &Render, x: f64, y: f64, rad: f64, rgb: Rgb, fade: f64) -> Res<()> {
        if rad < 0.5 {
            return Ok(());
        }
        let diameter = (2.0 * rad).round() as u16 + 2;
        let level = (fade * 16.0).round() as u8;
        let color = rgb
            .iter()
            .fold(0u32, |acc, v| acc << 8 | (v * 255.0).round() as u32);
        let key = (color, diameter, level);
        let cached = r.sprites.borrow().get(&key).copied();
        let (_, pic, d) = match cached {
            Some(s) => s,
            None => {
                let mut sprites = r.sprites.borrow_mut();
                if sprites.len() >= MAX_SPRITES {
                    for (_, (pm, pic, _)) in sprites.drain() {
                        self.conn.render_free_picture(pic)?;
                        self.conn.free_pixmap(pm)?;
                    }
                }
                let s = self.make_sprite(r, diameter, rgb, f64::from(level) / 16.0)?;
                sprites.insert(key, s);
                s
            }
        };
        let half = f64::from(d) / 2.0;
        self.conn.render_composite(
            PictOp::OVER,
            pic,
            x11rb::NONE,
            r.back,
            0,
            0,
            0,
            0,
            (x - half).round() as i16,
            (y - half).round() as i16,
            d,
            d,
        )?;
        Ok(())
    }

    /// Imagen ARGB transparente con la esfera: degradado radial (brillo arriba a la izquierda,
    /// borde oscuro) recortado por un círculo con bordes suavizados.
    fn make_sprite(&self, r: &Render, d: u16, rgb: Rgb, fade: f64) -> Res<(Pixmap, Picture, u16)> {
        let (pm, pic) = (self.conn.generate_id()?, self.conn.generate_id()?);
        self.conn.create_pixmap(32, pm, self.win, d, d)?;
        let aux = CreatePictureAux::new().polyedge(PolyEdge::SMOOTH);
        self.conn.render_create_picture(pic, pm, r.argb32, &aux)?;
        let clear = render::Color {
            red: 0,
            green: 0,
            blue: 0,
            alpha: 0,
        };
        let all = Rectangle {
            x: 0,
            y: 0,
            width: d,
            height: d,
        };
        self.conn
            .render_fill_rectangles(PictOp::SRC, pic, clear, &[all])?;
        let c = f64::from(d) / 2.0;
        let (x, y, rad) = (c, c, c - 1.0);
        let g = self.conn.generate_id()?;
        let highlight = (x - 0.38 * rad, y - 0.38 * rad);
        let k = |f: f64| shade(rgb, f * fade, 0.0);
        let shine = rgb.map(|v| (0.45 * v + 0.55) * fade);
        let stops = [0.0, 0.18, 0.7, 1.0].map(fx);
        let colors = [shine, k(1.05), k(0.65), k(0.3)].map(rcolor);
        self.conn.render_create_radial_gradient(
            g,
            pfx(highlight),
            pfx((x, y)),
            0,
            fx(rad),
            &stops,
            &colors,
        )?;
        let n = ((rad * 0.7) as usize).clamp(16, 72);
        let mut pts = Vec::with_capacity(n + 2);
        pts.push(pfx((x, y)));
        for i in 0..=n {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            pts.push(pfx((x + rad * a.cos(), y + rad * a.sin())));
        }
        // El origen del degradado coincide con el primer punto (truncado), como pide XRender.
        let (sx, sy) = ((pts[0].x >> 16) as i16, (pts[0].y >> 16) as i16);
        self.conn
            .render_tri_fan(PictOp::OVER, g, pic, r.a8, sx, sy, &pts)?;
        self.conn.render_free_picture(g)?;
        Ok((pm, pic, d))
    }

    /// Medio enlace suavizado: una textura de cilindro (dibujada una vez por color, grosor y
    /// profundidad) que se gira y estira con una transformación y se copia con filtro bilineal,
    /// que suaviza los bordes. Dos peticiones pequeñas por medio enlace. Rasterizar triángulos
    /// suavizados en cada fotograma era mucho más lento en el servidor (medido: de 1600 a 6–16 fps
    /// con 32 átomos).
    #[allow(clippy::too_many_arguments)]
    fn bond_aa(
        &self,
        r: &Render,
        a: (f64, f64),
        b: (f64, f64),
        w: f64,
        rgb: Rgb,
        fade: f64,
    ) -> Res<()> {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = dx.hypot(dy);
        if len < 0.5 || w < 0.5 {
            return Ok(());
        }
        let (ux, uy) = (dx / len, dy / len);
        // Un poco más allá del punto medio, para que no se vea la unión con la otra mitad.
        let len = len + 0.75;
        let thick = w.round().max(1.0) as u16;
        let level = (fade * 16.0).round() as u8;
        let color = rgb
            .iter()
            .fold(0u32, |acc, v| acc << 8 | (v * 255.0).round() as u32);
        let key = (color, thick, level);
        let cached = r.sticks.borrow().get(&key).copied();
        let (_, pic, h) = match cached {
            Some(t) => t,
            None => {
                let mut sticks = r.sticks.borrow_mut();
                if sticks.len() >= MAX_SPRITES {
                    for (_, (pm, pic, _)) in sticks.drain() {
                        self.conn.render_free_picture(pic)?;
                        self.conn.free_pixmap(pm)?;
                    }
                }
                let t = self.make_stick(r, thick, rgb, f64::from(level) / 16.0)?;
                sticks.insert(key, t);
                t
            }
        };
        // Normal que apunta del lado iluminado (arriba a la izquierda) al oscuro: la fila 0 de
        // la textura es el lado iluminado.
        let (mut nx, mut ny) = (-uy, ux);
        if nx + ny < 0.0 {
            (nx, ny) = (-nx, -ny);
        }
        let half = f64::from(h) / 2.0;
        let corners = [
            (a.0 + nx * half, a.1 + ny * half),
            (a.0 - nx * half, a.1 - ny * half),
            (a.0 + ux * len + nx * half, a.1 + uy * len + ny * half),
            (a.0 + ux * len - nx * half, a.1 + uy * len - ny * half),
        ];
        let x0 = corners.iter().map(|c| c.0).fold(f64::MAX, f64::min).floor() - 1.0;
        let y0 = corners.iter().map(|c| c.1).fold(f64::MAX, f64::min).floor() - 1.0;
        let x1 = corners.iter().map(|c| c.0).fold(f64::MIN, f64::max).ceil() + 1.0;
        let y1 = corners.iter().map(|c| c.1).fold(f64::MIN, f64::max).ceil() + 1.0;
        // Destino → textura, en coordenadas relativas a la esquina (x0, y0) del rectángulo:
        // u = (p − a)·dir · STICK_LEN / len,  v = (p − a)·n + h/2.
        let (ax, ay) = (a.0 - x0, a.1 - y0);
        let s = f64::from(STICK_LEN) / len;
        let t = render::Transform {
            matrix11: fx(s * ux),
            matrix12: fx(s * uy),
            matrix13: fx(-s * (ux * ax + uy * ay)),
            matrix21: fx(nx),
            matrix22: fx(ny),
            matrix23: fx(-(nx * ax + ny * ay) + half),
            matrix31: 0,
            matrix32: 0,
            matrix33: fx(1.0),
        };
        self.conn.render_set_picture_transform(pic, t)?;
        self.conn.render_composite(
            PictOp::OVER,
            pic,
            x11rb::NONE,
            r.back,
            0,
            0,
            0,
            0,
            x0 as i16,
            y0 as i16,
            (x1 - x0) as u16,
            (y1 - y0) as u16,
        )?;
        Ok(())
    }

    /// Textura de cilindro: `STICK_LEN` × (grosor + 2) px, filas de color según la normal del
    /// cilindro (fila 0 = lado iluminado) y una fila transparente arriba y abajo.
    fn make_stick(
        &self,
        r: &Render,
        thick: u16,
        rgb: Rgb,
        fade: f64,
    ) -> Res<(Pixmap, Picture, u16)> {
        let h = thick + 2;
        let (pm, pic) = (self.conn.generate_id()?, self.conn.generate_id()?);
        self.conn.create_pixmap(32, pm, self.win, STICK_LEN, h)?;
        self.conn
            .render_create_picture(pic, pm, r.argb32, &CreatePictureAux::new())?;
        let clear = render::Color {
            red: 0,
            green: 0,
            blue: 0,
            alpha: 0,
        };
        let all = Rectangle {
            x: 0,
            y: 0,
            width: STICK_LEN,
            height: h,
        };
        self.conn
            .render_fill_rectangles(PictOp::SRC, pic, clear, &[all])?;
        for row in 0..thick {
            // s ∈ (−1, 1): −1 lado iluminado, 1 lado oscuro; c = componente hacia el observador.
            let s = (f64::from(row) + 0.5) / f64::from(thick) * 2.0 - 1.0;
            let c = (1.0 - s * s).max(0.0).sqrt();
            let diffuse = (0.55 * -s + 0.83 * c).max(0.0);
            let spec = 0.35 * (0.3 * -s + 0.95 * c).max(0.0).powi(24);
            let k = (0.22 + 0.85 * diffuse) * fade;
            let line = Rectangle {
                x: 0,
                y: row as i16 + 1,
                width: STICK_LEN,
                height: 1,
            };
            let fill = rcolor(shade(rgb, k, spec * fade));
            self.conn
                .render_fill_rectangles(PictOp::SRC, pic, fill, &[line])?;
        }
        self.conn.render_set_picture_filter(pic, b"bilinear", &[])?;
        Ok((pm, pic, h))
    }

    /// Capa estática de la gráfica: fondo, ejes con marcas y valores, línea y puntos.
    fn draw_plot_layer(&self, app: &App) -> Res<()> {
        let c = &self.canvas;
        let d = self.plot;
        let l = &app.layout;
        let off = l.plot.y;
        let local = |r: Rect| Rect { y: r.y - off, ..r };
        c.rect(d, local(l.plot), BG_PLOT)?;
        c.rect(
            d,
            Rect {
                h: 1,
                ..local(l.plot)
            },
            GRID,
        )?;
        let Some(m) = app.plot_map() else {
            let msg = "No structure has an energy: nothing to plot.";
            c.set_font(self.font.id)?;
            return c.text(d, 20, l.plot.h / 2, msg, WEAK);
        };
        let a = local(m.area);
        let (cw, lh, asc) = (self.font.char_w, self.font.line_h, self.font.ascent);
        c.set_font(self.font.id)?;
        // Marcas de energía (eje y) con su valor.
        let (ticks, step) = nice_ticks(m.e.0, m.e.1, (a.h / (2 * lh)).max(2) as usize);
        let mut grid = Vec::new();
        for &e in &ticks {
            let y = (m.y(e) - f64::from(off)).round() as i32;
            grid.push(seg(a.x, y, a.x + a.w, y));
            let t = tick_text(e, step);
            c.text(d, a.x - 6 - cw * t.len() as i32, y + asc / 2 - 1, &t, WEAK)?;
        }
        // Marcas de n (eje x), enteras.
        let (nt, nstep) = nice_ticks(m.n.0, m.n.1, (a.w / (cw * 9)).max(2) as usize);
        for &n in nt.iter().filter(|&&n| n >= 1.0 && n.fract() == 0.0) {
            let x = m.x(n).round() as i32;
            grid.push(seg(x, a.y, x, a.y + a.h));
            let t = tick_text(n, nstep.max(1.0));
            c.text(
                d,
                x - cw * t.len() as i32 / 2,
                a.y + a.h + asc + 4,
                &t,
                WEAK,
            )?;
        }
        c.segments(d, &grid, 1, GRID)?;
        c.outline(
            d,
            Rect {
                x: a.x - 1,
                y: a.y - 1,
                w: a.w + 2,
                h: a.h + 2,
            },
            WEAK,
        )?;
        c.text(d, 8, a.y + asc, "E", TEXT)?;
        c.text(d, a.x + a.w + cw / 2, a.y + a.h + asc + 4, "n", TEXT)?;
        // Datos: línea y, si no están demasiado juntos, puntos.
        let pts: Vec<(f64, f64)> = app
            .loaded
            .structures
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                s.energy
                    .map(|e| (m.x((i + 1) as f64), m.y(e) - f64::from(off)))
            })
            .collect();
        let line: Vec<Segment> = pts
            .windows(2)
            .map(|w| seg_f(w[0], w[1]))
            .filter(|s| (s.x1, s.y1) != (s.x2, s.y2))
            .collect();
        c.segments(d, &line, 1, LINE)?;
        if pts.len() as i32 <= a.w / 4 {
            c.circles(d, &pts, 3.0, POINT)?;
        }
        Ok(())
    }

    fn draw_frame(&self, app: &mut App) -> Res<()> {
        let c = &self.canvas;
        let d = self.back;
        let l = app.layout.clone();
        let (cw, asc) = (self.font.char_w, self.font.ascent);

        // Vista 3D.
        c.rect(d, l.view, BG_VIEW)?;
        let smooth = self.render.as_ref().filter(|_| app.smooth);
        for shape in app.scene() {
            match (shape, smooth) {
                (Shape3d::Sphere { x, y, r, rgb, fade }, Some(rd)) => {
                    self.sphere_aa(rd, x, y, r, rgb, fade)?
                }
                (Shape3d::Sphere { x, y, r, rgb, fade }, None) => sphere(c, d, x, y, r, rgb, fade)?,
                (
                    Shape3d::Bond {
                        from,
                        to,
                        width,
                        rgb,
                        fade,
                    },
                    Some(rd),
                ) => self.bond_aa(rd, from, to, width, rgb, fade)?,
                (
                    Shape3d::Bond {
                        from,
                        to,
                        width,
                        rgb,
                        fade,
                    },
                    None,
                ) => bond(c, d, from, to, width, rgb, fade)?,
            }
        }
        c.set_font(self.font.id)?;
        let hint = "drag: rotate · right button: pan · wheel: zoom · double-click: reset";
        c.text(d, l.view.x + 8, l.view.y + l.view.h - 8, hint, WEAK)?;

        // Gráfica: capa estática + estructura actual + punto bajo el puntero.
        self.conn.copy_area(
            self.plot,
            d,
            c.gc,
            0,
            0,
            0,
            l.plot.y as i16,
            l.w.max(1) as u16,
            l.plot.h.max(1) as u16,
        )?;
        let structures = &app.loaded.structures;
        if let Some(m) = app.plot_map() {
            let cur = app.player.current;
            let x = m.x((cur + 1) as f64);
            let (top, bottom) = (f64::from(m.area.y), f64::from(m.area.y + m.area.h));
            c.line(d, (x, top), (x, bottom), 1, ACCENT)?;
            if let Some(e) = structures[cur].energy {
                c.circles(d, &[(x, m.y(e))], 6.0, ACCENT)?;
            }
            if let Hover::Plot(i) = app.hover
                && let Some(e) = structures[i].energy
            {
                let p = (m.x((i + 1) as f64), m.y(e));
                c.circles(d, &[p], 4.5, TEXT)?;
                let s = &structures[i];
                let mut lines = vec![format!("n = {}", i + 1), format!("E = {e}")];
                if !s.label.is_empty() {
                    lines.push(s.label.clone());
                }
                self.tooltip(app, p.0 as i32, p.1 as i32, &lines)?;
            }
        }

        // Cabecera.
        c.rect(d, l.header, BG_PANEL)?;
        let s = app.structure();
        let y = l.header.y + (l.header.h + asc) / 2 - 1;
        let mut x = 10;
        let mut put = |c: &Canvas, font: &FontInfo, text: &str, rgb: Rgb| -> Res<()> {
            c.set_font(font.id)?;
            c.text(d, x, y, text, rgb)?;
            x += font.char_w * latin1(text).len() as i32 + 2 * cw;
            Ok(())
        };
        put(
            c,
            &self.bold,
            &format!("{} / {}", app.player.current + 1, app.player.len),
            TEXT,
        )?;
        put(c, &self.bold, &s.formula(), TEXT)?;
        put(c, &self.font, &format!("{} atoms", s.atoms.len()), TEXT)?;
        match s.energy {
            Some(e) => put(c, &self.bold, &format!("E = {e}"), TEXT)?,
            None => put(c, &self.font, "no energy", WARN)?,
        }
        if !s.label.is_empty() {
            put(c, &self.font, &s.label, TEXT)?;
        }
        let file = s.path.file_name().unwrap_or_default().to_string_lossy();
        put(c, &self.font, &format!("{file}:{}", s.line), WEAK)?;
        let n = app.loaded.warnings.len();
        if n > 0 {
            let plural = if n == 1 { "" } else { "s" };
            put(
                c,
                &self.font,
                &format!("! {n} warning{plural} (see terminal)"),
                WARN,
            )?;
        }

        // Barra de controles.
        c.rect(d, l.bar, BG_PANEL)?;
        c.rect(d, Rect { h: 1, ..l.bar }, GRID)?;
        c.set_font(self.font.id)?;
        for &(item, r) in &l.items {
            let ty = r.y + (r.h + asc) / 2 - 1;
            match item {
                BarItem::Speed => c.text(d, r.x + 2, ty, &speed_text(app.player.speed), TEXT)?,
                BarItem::Tolerance => {
                    c.text(d, r.x + 2, ty, &tolerance_text(app.bond_tolerance), TEXT)?
                }
                BarItem::Button(b) => self.button(app, b, r)?,
            }
        }

        // Átomo bajo el puntero.
        if let Hover::Atom(i) = app.hover {
            let a = &app.structure().atoms[i];
            let text = format!(
                "{} {}   ({:.4}, {:.4}, {:.4}) Å",
                elements::symbol(a.z),
                i + 1,
                a.pos[0],
                a.pos[1],
                a.pos[2]
            );
            self.tooltip(app, app.pointer.0, app.pointer.1, &[text])?;
        }
        Ok(())
    }

    fn button(&self, app: &App, b: Button, r: Rect) -> Res<()> {
        let c = &self.canvas;
        let d = self.back;
        let hovered = app.hover == Hover::Button(b);
        let checkbox = match b {
            Button::Loop => Some((app.player.looping, "Loop")),
            Button::MetalMetal => Some((app.metal_metal, "Metal-metal bonds")),
            Button::Smooth => Some((app.smooth, "Smooth")),
            _ => None,
        };
        let (cw, asc) = (self.font.char_w, self.font.ascent);
        let ty = r.y + (r.h + asc) / 2 - 1;
        if let Some((on, label)) = checkbox {
            let s = r.h - 8;
            let bx = Rect {
                x: r.x + 2,
                y: r.y + 4,
                w: s,
                h: s,
            };
            c.rect(d, bx, if on { ACCENT } else { [0.2, 0.2, 0.22] })?;
            c.outline(d, bx, if hovered { TEXT } else { WEAK })?;
            if on {
                let (x0, y0) = (bx.x + 3, bx.y + s / 2);
                let tick = [
                    seg(x0, y0, x0 + s / 4, bx.y + s - 4),
                    seg(x0 + s / 4, bx.y + s - 4, bx.x + s - 3, bx.y + 3),
                ];
                c.segments(d, &tick, 2, TEXT)?;
            }
            return c.text(d, bx.x + s + cw / 2 + 2, ty, label, TEXT);
        }
        let face = if hovered {
            [0.26, 0.26, 0.3]
        } else {
            [0.19, 0.19, 0.21]
        };
        c.rect(d, r, face)?;
        c.outline(d, r, [0.32, 0.32, 0.36])?;
        let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
        let k = (r.h / 4).max(3);
        let right = |x: i32| [(x - k, cy - k), (x + k, cy), (x - k, cy + k)];
        let left = |x: i32| [(x + k, cy - k), (x - k, cy), (x + k, cy + k)];
        let bar = |x: i32| Rect {
            x,
            y: cy - k,
            w: 2,
            h: 2 * k + 1,
        };
        match b {
            Button::First => {
                c.rect(d, bar(cx - k - 3), TEXT)?;
                c.polygon(d, &left(cx + 1), TEXT)?;
            }
            Button::Prev => c.polygon(d, &left(cx), TEXT)?,
            Button::Play if app.player.playing => {
                c.rect(
                    d,
                    Rect {
                        x: cx - k + 1,
                        y: cy - k,
                        w: k - 1,
                        h: 2 * k + 1,
                    },
                    TEXT,
                )?;
                c.rect(
                    d,
                    Rect {
                        x: cx + 1,
                        y: cy - k,
                        w: k - 1,
                        h: 2 * k + 1,
                    },
                    TEXT,
                )?;
            }
            Button::Play => c.polygon(d, &right(cx + 1), ACCENT)?,
            Button::Next => c.polygon(d, &right(cx), TEXT)?,
            Button::Last => {
                c.polygon(d, &right(cx - 1), TEXT)?;
                c.rect(d, bar(cx + k + 2), TEXT)?;
            }
            Button::Slower | Button::LessTolerance => c.rect(
                d,
                Rect {
                    x: cx - k,
                    y: cy,
                    w: 2 * k + 1,
                    h: 2,
                },
                TEXT,
            )?,
            Button::Faster | Button::MoreTolerance => {
                c.rect(
                    d,
                    Rect {
                        x: cx - k,
                        y: cy,
                        w: 2 * k + 1,
                        h: 2,
                    },
                    TEXT,
                )?;
                c.rect(
                    d,
                    Rect {
                        x: cx,
                        y: cy - k,
                        w: 2,
                        h: 2 * k + 1,
                    },
                    TEXT,
                )?;
            }
            Button::Loop | Button::MetalMetal | Button::Smooth => {}
        }
        Ok(())
    }

    /// Recuadro con texto junto a (x, y), dentro de la ventana.
    fn tooltip(&self, app: &App, x: i32, y: i32, lines: &[String]) -> Res<()> {
        let c = &self.canvas;
        let (cw, lh, asc) = (self.font.char_w, self.font.line_h, self.font.ascent);
        let w = lines
            .iter()
            .map(|t| latin1(t).len() as i32)
            .max()
            .unwrap_or(0)
            * cw
            + 12;
        let h = lines.len() as i32 * lh + 8;
        let l = &app.layout;
        let bx = (x + 14).min(l.w - w - 2).max(2);
        let by = (y - h - 10).max(2);
        let r = Rect { x: bx, y: by, w, h };
        c.rect(self.back, r, [0.16, 0.16, 0.18])?;
        c.outline(self.back, r, WEAK)?;
        c.set_font(self.font.id)?;
        for (k, t) in lines.iter().enumerate() {
            c.text(self.back, bx + 6, by + 4 + asc + k as i32 * lh, t, TEXT)?;
        }
        Ok(())
    }
}

fn seg(x1: i32, y1: i32, x2: i32, y2: i32) -> Segment {
    Segment {
        x1: x1 as i16,
        y1: y1 as i16,
        x2: x2 as i16,
        y2: y2 as i16,
    }
}

fn seg_f(a: (f64, f64), b: (f64, f64)) -> Segment {
    seg(
        a.0.round() as i32,
        a.1.round() as i32,
        b.0.round() as i32,
        b.1.round() as i32,
    )
}

fn shade(rgb: Rgb, k: f64, spec: f64) -> Rgb {
    rgb.map(|v| v * k + spec)
}

/// Esfera: borde oscuro y círculos concéntricos cada vez más claros, desplazados hacia la luz
/// (arriba a la izquierda), con un brillo final.
fn sphere(c: &Canvas, d: Drawable, x: f64, y: f64, r: f64, rgb: Rgb, fade: f64) -> Res<()> {
    let rings = (r / 2.5).clamp(5.0, 16.0) as usize;
    c.circles(d, &[(x, y)], r, shade(rgb, 0.18 * fade, 0.0))?;
    for k in 0..rings {
        let t = k as f64 / rings as f64;
        let rk = r * (0.96 - 0.82 * t);
        let o = 0.32 * r * t;
        let light = (0.32 + 0.78 * t.powf(0.7)) * fade;
        let spec = if k + 1 == rings { 0.3 * fade } else { 0.0 };
        c.circles(d, &[(x - o, y - o)], rk, shade(rgb, light, spec))?;
    }
    Ok(())
}

/// Medio enlace: trazo oscuro, uno medio y un reflejo claro hacia la luz.
fn bond(
    c: &Canvas,
    d: Drawable,
    a: (f64, f64),
    b: (f64, f64),
    w: f64,
    rgb: Rgb,
    fade: f64,
) -> Res<()> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = dx.hypot(dy);
    if len < 0.5 {
        return Ok(());
    }
    let (px, py) = (-dy / len, dx / len);
    // Lado de la normal que mira a la luz (arriba a la izquierda).
    let side = if px + py > 0.0 { -1.0 } else { 1.0 };
    for (frac, off, k) in [(1.0, 0.0, 0.42), (0.6, 0.12, 0.8), (0.22, 0.22, 1.15)] {
        let o = side * off * w;
        let (ox, oy) = (px * o, py * o);
        let width = (w * frac).round().max(1.0) as u32;
        c.line(
            d,
            (a.0 + ox, a.1 + oy),
            (b.0 + ox, b.1 + oy),
            width,
            shade(rgb, k * fade, 0.0),
        )?;
    }
    Ok(())
}

/// Abre la ventana y atiende eventos hasta que se cierra.
pub fn run(loaded: Loaded, title: &str, options: Options) -> Result<(), String> {
    run_inner(loaded, title, options).map_err(|e| e.to_string())
}

fn run_inner(loaded: Loaded, title: &str, options: Options) -> Res<()> {
    let (conn, screen_num) =
        x11rb::connect(None).map_err(|e| format!("cannot connect to the X display: {e}"))?;
    let screen = conn.setup().roots[screen_num].clone();
    let visual = screen
        .allowed_depths
        .iter()
        .flat_map(|d| &d.visuals)
        .find(|v| v.visual_id == screen.root_visual)
        .ok_or("root visual not found")?;
    if visual.class != VisualClass::TRUE_COLOR && visual.class != VisualClass::DIRECT_COLOR {
        return Err("a TrueColor display is required".into());
    }
    let masks = [visual.red_mask, visual.green_mask, visual.blue_mask];
    let font = open_font(&conn, &["9x15", "8x13", "fixed"])?;
    let bold = open_font(&conn, &["9x15bold", "8x13bold"]).unwrap_or(font);
    let (w, h) = (1200, 820);
    let mut app = App::new(loaded, Layout::new(w, h, font.char_w, font.line_h));
    let mut render = init_render(&conn, screen_num, screen.root_visual)?;
    if let Some(r) = &mut render {
        r.back = conn.generate_id()?;
    }
    app.smooth = options.smooth && render.is_some();
    if options.smooth && render.is_none() {
        eprintln!("mole: warning: the X server has no XRender >= 0.10; using basic drawing");
    }

    let win = conn.generate_id()?;
    conn.create_window(
        x11rb::COPY_DEPTH_FROM_PARENT,
        win,
        screen.root,
        0,
        0,
        w as u16,
        h as u16,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new()
            .background_pixel(screen.black_pixel)
            .event_mask(
                EventMask::EXPOSURE
                    | EventMask::STRUCTURE_NOTIFY
                    | EventMask::BUTTON_PRESS
                    | EventMask::BUTTON_RELEASE
                    | EventMask::POINTER_MOTION
                    | EventMask::LEAVE_WINDOW
                    | EventMask::KEY_PRESS,
            ),
    )?;
    let atom = |name: &[u8]| -> Res<Atom> { Ok(conn.intern_atom(false, name)?.reply()?.atom) };
    let (wm_protocols, wm_delete) = (atom(b"WM_PROTOCOLS")?, atom(b"WM_DELETE_WINDOW")?);
    let (net_wm_name, utf8) = (atom(b"_NET_WM_NAME")?, atom(b"UTF8_STRING")?);
    conn.change_property8(
        PropMode::REPLACE,
        win,
        AtomEnum::WM_NAME,
        AtomEnum::STRING,
        &latin1(title),
    )?;
    conn.change_property8(PropMode::REPLACE, win, net_wm_name, utf8, title.as_bytes())?;
    conn.change_property8(
        PropMode::REPLACE,
        win,
        AtomEnum::WM_CLASS,
        AtomEnum::STRING,
        b"mole\0MOLE\0",
    )?;
    conn.change_property32(
        PropMode::REPLACE,
        win,
        wm_protocols,
        AtomEnum::ATOM,
        &[wm_delete],
    )?;
    conn.map_window(win)?;

    let gc = conn.generate_id()?;
    conn.create_gc(
        gc,
        win,
        &CreateGCAux::new()
            .graphics_exposures(0)
            .cap_style(CapStyle::BUTT)
            .font(font.id),
    )?;
    let window = Window {
        conn: &conn,
        win,
        back: conn.generate_id()?,
        plot: conn.generate_id()?,
        depth: screen.root_depth,
        canvas: Canvas {
            conn: &conn,
            gc,
            masks,
            fg: Cell::new(None),
            width: Cell::new(0),
            font: Cell::new(font.id),
        },
        font,
        bold,
        render,
    };
    window.make_pixmaps(&app.layout)?;

    // Teclado: keycode → keysym (primera columna).
    let setup = conn.setup();
    let (min_kc, max_kc) = (setup.min_keycode, setup.max_keycode);
    let map = conn
        .get_keyboard_mapping(min_kc, max_kc - min_kc + 1)?
        .reply()?;
    let per = usize::from(map.keysyms_per_keycode);
    let keysym = |kc: u8| {
        map.keysyms
            .get(usize::from(kc - min_kc) * per)
            .copied()
            .unwrap_or(0)
    };

    let bench: Option<usize> = std::env::var("MOLE_BENCH")
        .ok()
        .and_then(|n| n.parse().ok());
    #[cfg(debug_assertions)]
    let capture = std::env::var("MOLE_CAPTURE").ok();
    #[cfg(debug_assertions)]
    if let Some(n) = std::env::var("MOLE_CAPTURE_N")
        .ok()
        .and_then(|n| n.parse::<usize>().ok())
    {
        app.player.go(n.saturating_sub(1));
    }

    let t0 = Instant::now();
    let now = || t0.elapsed().as_secs_f64();
    let (mut dirty, mut plot_dirty) = (true, true);
    let mut pending_sync = None;
    let mut frames = 0usize;
    let mut bench_start = 0.0;
    loop {
        // Atender todos los eventos pendientes y dibujar una sola vez; sin nada que hacer, esperar.
        let before = app.player.current;
        let wait = app.player.tick(now());
        dirty |= app.player.current != before;
        let mut event = if dirty || bench.is_some() {
            conn.poll_for_event()?
        } else if let Some(wait) = wait {
            std::thread::sleep(std::time::Duration::from_secs_f64(wait.min(0.01)));
            conn.poll_for_event()?
        } else {
            Some(conn.wait_for_event()?)
        };
        while let Some(e) = event {
            let input = match e {
                Event::Expose(x) if x.count == 0 => {
                    dirty = true;
                    None
                }
                Event::ConfigureNotify(c)
                    if (i32::from(c.width), i32::from(c.height))
                        != (app.layout.w, app.layout.h) =>
                {
                    window.free_pixmaps()?;
                    app.handle(
                        Input::Resize {
                            w: c.width.into(),
                            h: c.height.into(),
                        },
                        now(),
                    );
                    window.make_pixmaps(&app.layout)?;
                    plot_dirty = true;
                    None
                }
                Event::ButtonPress(b) => Some(Input::Press {
                    x: b.event_x.into(),
                    y: b.event_y.into(),
                    button: b.detail,
                }),
                Event::ButtonRelease(b) => Some(Input::Release { button: b.detail }),
                Event::MotionNotify(m) => Some(Input::Motion {
                    x: m.event_x.into(),
                    y: m.event_y.into(),
                }),
                Event::LeaveNotify(_) => Some(Input::Leave),
                Event::KeyPress(k) => match keysym(k.detail) {
                    0xff51 => Some(Input::Key(Key::Left)),
                    0xff53 => Some(Input::Key(Key::Right)),
                    0xff50 => Some(Input::Key(Key::Home)),
                    0xff57 => Some(Input::Key(Key::End)),
                    0x20 => Some(Input::Key(Key::Space)),
                    0x71 | 0xff1b => Some(Input::Key(Key::Quit)),
                    _ => None,
                },
                Event::ClientMessage(m) if m.data.as_data32()[0] == wm_delete => {
                    Some(Input::Key(Key::Quit))
                }
                _ => None,
            };
            if let Some(input) = input {
                dirty |= app.handle(input, now());
            }
            event = conn.poll_for_event()?;
        }
        if app.quit {
            return Ok(());
        }
        if let Some(n) = bench {
            if frames == 0 {
                bench_start = now();
            }
            app.player.next();
            dirty = true;
            if frames == n {
                if let Some(prev) = pending_sync.take() {
                    x11rb::cookie::Cookie::reply(prev)?;
                }
                let fps = n as f64 / (now() - bench_start);
                eprintln!(
                    "mole: bench: {fps:.1} fps over {n} frames, window {}x{} (X11)",
                    app.layout.w, app.layout.h
                );
                return Ok(());
            }
        }
        if !dirty {
            continue;
        }
        if plot_dirty {
            window.draw_plot_layer(&app)?;
            plot_dirty = false;
        }
        window.draw_frame(&mut app)?;
        let l = &app.layout;
        conn.copy_area(window.back, win, gc, 0, 0, 0, 0, l.w as u16, l.h as u16)?;
        // Como mucho un fotograma en vuelo: si la red va lenta no se acumulan órdenes.
        let sync = conn.get_input_focus()?;
        if let Some(prev) = pending_sync.replace(sync) {
            x11rb::cookie::Cookie::reply(prev)?;
        }
        conn.flush()?;
        dirty = false;
        frames += 1;
        #[cfg(debug_assertions)]
        if let Some(path) = &capture
            && frames == 1
        {
            save_capture(&conn, window.back, &app.layout, masks, path)?;
            return Ok(());
        }
    }
}

/// Solo en compilaciones de depuración: `MOLE_CAPTURE=salida.ppm [MOLE_CAPTURE_N=k]` guarda el
/// fotograma y cierra, para revisar la interfaz sin interacción.
#[cfg(debug_assertions)]
fn save_capture(
    conn: &RustConnection,
    back: Pixmap,
    l: &Layout,
    masks: [u32; 3],
    path: &str,
) -> Res<()> {
    let img = conn
        .get_image(
            ImageFormat::Z_PIXMAP,
            back,
            0,
            0,
            l.w as u16,
            l.h as u16,
            !0,
        )?
        .reply()?;
    let mut ppm = format!("P6\n{} {}\n255\n", l.w, l.h).into_bytes();
    for px in img.data.as_chunks::<4>().0 {
        let v = u32::from_le_bytes(*px);
        ppm.extend(masks.map(|m| ((v & m) >> m.trailing_zeros()) as u8));
    }
    std::fs::write(path, ppm)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::latin1;

    #[test]
    fn latin1_replaces_what_server_fonts_cannot_show() {
        assert_eq!(latin1("Å·x"), vec![0xC5, 0xB7, b'x']);
        assert_eq!(latin1("a–b"), b"a?b".to_vec());
    }
}
