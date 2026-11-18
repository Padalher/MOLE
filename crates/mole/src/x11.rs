use crate::app::{
    App, BarItem, Button, Edge, FIX_ORIENTATION_LABEL, FORCES_LABEL, Hover, Input, Key,
    LIGHT_LABEL, Layout, METAL_METAL_LABEL, RELATIVE_ENERGY_LABEL, Rect, Shape3d,
    TRANSPARENT_LABEL, column_lines, iso_text, nice_ticks, playback_label, short_number,
    speed_text, tick_exponent, tick_text, tolerance_text,
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

struct Theme {
    view: Rgb,
    panel: Rgb,
    plot: Rgb,
    text: Rgb,
    weak: Rgb,
    grid: Rgb,
    unchecked: Rgb,
    face: Rgb,
    face_hover: Rgb,
    border: Rgb,
    tooltip: Rgb,
    warn: Rgb,
    force: Rgb,
}

const DARK: Theme = Theme {
    view: [0.04, 0.04, 0.045],
    panel: [0.11, 0.11, 0.12],
    plot: [0.07, 0.07, 0.08],
    text: [0.9, 0.9, 0.92],
    weak: [0.55, 0.55, 0.6],
    grid: [0.18, 0.18, 0.2],
    unchecked: [0.2, 0.2, 0.22],
    face: [0.19, 0.19, 0.21],
    face_hover: [0.26, 0.26, 0.3],
    border: [0.32, 0.32, 0.36],
    tooltip: [0.16, 0.16, 0.18],
    warn: [1.0, 0.65, 0.2],
    force: [0.95, 0.65, 0.05],
};

const LIGHT: Theme = Theme {
    view: [1.0, 1.0, 1.0],
    panel: [0.94, 0.94, 0.95],
    plot: [1.0, 1.0, 1.0],
    text: [0.1, 0.1, 0.12],
    weak: [0.4, 0.4, 0.45],
    grid: [0.85, 0.85, 0.88],
    unchecked: [1.0, 1.0, 1.0],
    face: [0.98, 0.98, 0.99],
    face_hover: [0.86, 0.89, 0.95],
    border: [0.7, 0.7, 0.75],
    tooltip: [1.0, 1.0, 0.92],
    warn: [0.75, 0.42, 0.0],
    force: [0.8, 0.45, 0.0],
};

fn theme(app: &App) -> &'static Theme {
    if app.light { &LIGHT } else { &DARK }
}

const SEE_THROUGH: f64 = 0.45;

const ACCENT: Rgb = [0.2, 0.6, 0.95];
const LINE: Rgb = [0.85, 0.35, 0.3];
const POINT: Rgb = [0.45, 0.6, 0.95];

fn latin1(s: &str) -> Vec<u8> {
    s.chars()
        .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
        .collect()
}

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
        let Some((a, b)) = clip(a, b) else {
            return Ok(());
        };
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
                x: x.clamp(-32000, 32000) as i16,
                y: y.clamp(-32000, 32000) as i16,
            })
            .collect();
        self.conn
            .fill_poly(d, self.gc, PolyShape::COMPLEX, CoordMode::ORIGIN, &pts)?;
        Ok(())
    }

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

#[derive(Clone, Copy)]
struct FontInfo {
    id: Font,
    char_w: i32,
    ascent: i32,
    line_h: i32,
}

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
    Err(format!("no font available on the X server: {names:?}").into())
}

struct Render {
    back: Picture,
    layer: Pixmap,
    layer_picture: Picture,
    layer_mask: Picture,
    format: Pictformat,
    argb32: Pictformat,
    a8: Pictformat,
    sprites: RefCell<SpriteCache>,
    sticks: RefCell<SpriteCache>,
}

type SpriteCache = HashMap<(u32, u16, u8), (Pixmap, Picture, u16)>;

const STICK_LEN: u16 = 16;
const SPRITE_MAX: u16 = 256;
const STICK_MAX: u16 = 64;

const MAX_SPRITES: usize = 256;

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
            layer: 0,
            layer_picture: 0,
            layer_mask: 0,
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
            self.conn
                .create_pixmap(self.depth, r.layer, self.win, w, h)?;
            self.conn.render_create_picture(
                r.layer_picture,
                r.layer,
                r.format,
                &CreatePictureAux::new(),
            )?;
        }
        Ok(())
    }

    fn free_pixmaps(&self) -> Res<()> {
        if let Some(r) = &self.render {
            self.conn.render_free_picture(r.back)?;
            self.conn.render_free_picture(r.layer_picture)?;
            self.conn.free_pixmap(r.layer)?;
        }
        self.conn.free_pixmap(self.back)?;
        self.conn.free_pixmap(self.plot)?;
        Ok(())
    }

    fn sphere_aa(&self, r: &Render, x: f64, y: f64, rad: f64, rgb: Rgb, fade: f64) -> Res<()> {
        if rad < 0.5 {
            return Ok(());
        }
        let diameter = (2.0 * rad).round().min(8192.0) as u16 + 2;
        let size = diameter.min(SPRITE_MAX);
        let level = (fade * 16.0).round() as u8;
        let color = rgb
            .iter()
            .fold(0u32, |acc, v| acc << 8 | (v * 255.0).round() as u32);
        let key = (color, size, level);
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
                let s = self.make_sprite(r, size, rgb, f64::from(level) / 16.0)?;
                sprites.insert(key, s);
                s
            }
        };
        if d == SPRITE_MAX {
            let s = fx(f64::from(d) / f64::from(diameter));
            let t = render::Transform {
                matrix11: s,
                matrix12: 0,
                matrix13: 0,
                matrix21: 0,
                matrix22: s,
                matrix23: 0,
                matrix31: 0,
                matrix32: 0,
                matrix33: fx(1.0),
            };
            self.conn.render_set_picture_transform(pic, t)?;
        }
        let d = diameter;
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
        let (sx, sy) = ((pts[0].x >> 16) as i16, (pts[0].y >> 16) as i16);
        self.conn
            .render_tri_fan(PictOp::OVER, g, pic, r.a8, sx, sy, &pts)?;
        self.conn.render_free_picture(g)?;
        self.conn.render_set_picture_filter(pic, b"bilinear", &[])?;
        Ok((pm, pic, d))
    }

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
        let len = len + 0.75;
        let thick = w.round().clamp(1.0, 8192.0) as u16;
        let level = (fade * 16.0).round() as u8;
        let color = rgb
            .iter()
            .fold(0u32, |acc, v| acc << 8 | (v * 255.0).round() as u32);
        let key = (color, thick.min(STICK_MAX), level);
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
                let t = self.make_stick(r, thick.min(STICK_MAX), rgb, f64::from(level) / 16.0)?;
                sticks.insert(key, t);
                t
            }
        };
        let (mut nx, mut ny) = (-uy, ux);
        if nx + ny < 0.0 {
            (nx, ny) = (-nx, -ny);
        }
        let half = (f64::from(thick) + 2.0) / 2.0;
        let k = f64::from(h) / (2.0 * half);
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
        let (ax, ay) = (a.0 - x0, a.1 - y0);
        let s = f64::from(STICK_LEN) / len;
        let t = render::Transform {
            matrix11: fx(s * ux),
            matrix12: fx(s * uy),
            matrix13: fx(-s * (ux * ax + uy * ay)),
            matrix21: fx(k * nx),
            matrix22: fx(k * ny),
            matrix23: fx(-k * (nx * ax + ny * ay) + f64::from(h) / 2.0),
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

    fn draw_plot_layer(&self, app: &App) -> Res<()> {
        let th = theme(app);
        let c = &self.canvas;
        let d = self.plot;
        let l = &app.layout;
        let off = l.plot.y;
        let local = |r: Rect| Rect { y: r.y - off, ..r };
        c.rect(d, local(l.plot), th.plot)?;
        c.rect(
            d,
            Rect {
                h: 1,
                ..local(l.plot)
            },
            th.grid,
        )?;
        let Some(m) = app.plot_map() else {
            let msg = "No structure has an energy: nothing to plot.";
            c.set_font(self.font.id)?;
            return c.text(d, 20, l.plot.h / 2, msg, th.weak);
        };
        let a = local(m.area);
        let (cw, lh, asc) = (self.font.char_w, self.font.line_h, self.font.ascent);
        c.set_font(self.font.id)?;
        let r = m.e_ref.unwrap_or(0.0);
        let (ticks, step) = nice_ticks(m.e.0 - r, m.e.1 - r, (a.h / (2 * lh)).max(1) as usize);
        let k = tick_exponent(&ticks, step);
        let f = 10f64.powi(k);
        let mut grid = Vec::new();
        for &e in &ticks {
            let y = (m.y(e + r) - f64::from(off)).round() as i32;
            grid.push(seg(a.x, y, a.x + a.w, y));
            let t = tick_text(e / f, step / f);
            c.text(
                d,
                a.x - 6 - cw * t.len() as i32,
                y + asc / 2 - 1,
                &t,
                th.weak,
            )?;
        }
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
                th.weak,
            )?;
        }
        c.segments(d, &grid, 1, th.grid)?;
        c.outline(
            d,
            Rect {
                x: a.x - 1,
                y: a.y - 1,
                w: a.w + 2,
                h: a.h + 2,
            },
            th.weak,
        )?;
        let times = if k == 0 {
            String::new()
        } else {
            format!(" (×10^{k})")
        };
        match m.e_ref {
            Some(e_min) => {
                let label = format!("E - Emin{times}    (Emin = {e_min})");
                c.text(d, 8, a.y - lh / 2 - 4, &label, th.text)?
            }
            None => c.text(d, 8, a.y + asc, &format!("E{times}"), th.text)?,
        }
        c.text(d, a.x + a.w + cw / 2, a.y + a.h + asc + 4, "n", th.text)?;
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

    fn draw_shapes<'s>(
        &self,
        shapes: impl Iterator<Item = &'s Shape3d>,
        smooth: Option<&Render>,
        force: Rgb,
    ) -> Res<()> {
        let (c, d) = (&self.canvas, self.back);
        for &shape in shapes {
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
                (Shape3d::Quad { pts, rgb }, _) => c.polygon(
                    d,
                    &pts.map(|(x, y)| (x.round() as i32, y.round() as i32)),
                    rgb,
                )?,
                (Shape3d::Arrow { from, to, head }, _) => {
                    c.line(d, from, to, 3, force)?;
                    c.polygon(
                        d,
                        &head.map(|(x, y)| (x.round() as i32, y.round() as i32)),
                        force,
                    )?
                }
            }
        }
        Ok(())
    }

    fn draw_frame(&self, app: &mut App) -> Res<()> {
        let th = theme(app);
        let c = &self.canvas;
        let d = self.back;
        let l = app.layout.clone();
        let (cw, asc) = (self.font.char_w, self.font.ascent);

        c.rect(d, l.view, th.view)?;
        let smooth = self.render.as_ref().filter(|_| app.smooth);
        let scene = app.scene();
        let is_face = |s: &Shape3d| matches!(s, Shape3d::Quad { .. });
        let layer = smooth.filter(|_| app.transparent && scene.iter().any(is_face));
        if let Some(r) = layer {
            self.draw_shapes(scene.iter().filter(|s| !is_face(s)), smooth, th.force)?;
            let v = l.view;
            let (x, y) = (v.x as i16, v.y as i16);
            let (w, h) = (v.w as u16, v.h as u16);
            self.conn.copy_area(d, r.layer, c.gc, x, y, x, y, w, h)?;
            c.rect(d, v, th.view)?;
        }
        self.draw_shapes(scene.iter(), smooth, th.force)?;
        if let Some(r) = layer {
            let v = l.view;
            let (x, y) = (v.x as i16, v.y as i16);
            self.conn.render_composite(
                PictOp::OVER,
                r.layer_picture,
                r.layer_mask,
                r.back,
                x,
                y,
                0,
                0,
                x,
                y,
                v.w as u16,
                v.h as u16,
            )?;
        }
        c.set_font(self.font.id)?;
        let hint = "drag: rotate · right button: pan · wheel: zoom · double-click: reset";
        c.text(d, l.view.x + 8, l.view.y + l.view.h - 8, hint, th.weak)?;

        c.rect(d, l.panel, th.panel)?;
        c.rect(d, Rect { w: 1, ..l.panel }, th.grid)?;
        for &(item, r) in &l.items {
            let ty = r.y + (r.h + asc) / 2 - 1;
            let text = match item {
                BarItem::Speed => speed_text(app.player.speed),
                BarItem::Tolerance => tolerance_text(app.bond_tolerance),
                BarItem::Iso => iso_text(app.iso),
                BarItem::Title(t) => {
                    c.text(d, r.x + 2, r.y + asc, t, th.weak)?;
                    continue;
                }
                BarItem::Button(b) => {
                    self.button(app, b, r)?;
                    continue;
                }
            };
            let tx = r.x + (r.w - cw * text.len() as i32) / 2;
            c.text(d, tx, ty, &text, th.text)?;
        }

        self.conn.copy_area(
            self.plot,
            d,
            c.gc,
            0,
            0,
            0,
            l.plot.y as i16,
            l.plot.w.max(1) as u16,
            l.plot.h.max(1) as u16,
        )?;
        if let Hover::Edge(edge) = app.hover {
            let strip = match edge {
                Edge::Plot => Rect {
                    y: l.plot.y - 1,
                    h: 3,
                    ..l.plot
                },
                Edge::Panel => Rect {
                    x: l.panel.x - 1,
                    w: 3,
                    ..l.panel
                },
            };
            c.rect(d, strip, ACCENT)?;
        }
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
                c.circles(d, &[p], 4.5, th.text)?;
                let s = &structures[i];
                let mut lines = vec![format!("n = {}", i + 1), format!("E = {e}")];
                if let Some(e_min) = m.e_ref {
                    lines.push(format!("E - Emin = {}", short_number(e - e_min)));
                }
                if !s.label.is_empty() {
                    lines.push(s.label.clone());
                }
                self.tooltip(app, p.0 as i32, p.1 as i32, &lines)?;
            }
        }

        c.rect(d, l.header, th.panel)?;
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
            th.text,
        )?;
        put(c, &self.bold, &s.formula(), th.text)?;
        put(c, &self.font, &format!("{} atoms", s.atoms.len()), th.text)?;
        match s.energy {
            Some(e) => put(c, &self.bold, &format!("E = {e}"), th.text)?,
            None if s.grid.is_some() => {}
            None => put(c, &self.font, "no energy", th.warn)?,
        }
        if !s.label.is_empty() {
            put(c, &self.font, &s.label, th.text)?;
        }
        let file = s.path.file_name().unwrap_or_default().to_string_lossy();
        put(c, &self.font, &format!("{file}:{}", s.line), th.weak)?;
        let n = app.loaded.warnings.len();
        if n > 0 {
            let plural = if n == 1 { "" } else { "s" };
            put(
                c,
                &self.font,
                &format!("! {n} warning{plural} (see terminal)"),
                th.warn,
            )?;
        }

        if let Hover::Atom(i) = app.hover
            && let Some(a) = app.structure().atoms.get(i)
        {
            let text = format!(
                "{} {}   ({:.4}, {:.4}, {:.4}) Å",
                elements::symbol(a.z),
                i + 1,
                a.pos[0],
                a.pos[1],
                a.pos[2]
            );
            let mut lines = vec![text];
            lines.extend(column_lines(app.structure(), i));
            self.tooltip(app, app.pointer.0, app.pointer.1, &lines)?;
        }
        Ok(())
    }

    fn button(&self, app: &App, b: Button, r: Rect) -> Res<()> {
        let th = theme(app);
        let c = &self.canvas;
        let d = self.back;
        let hovered = app.hover == Hover::Button(b);
        let checkbox = match b {
            Button::Loop => Some((app.player.looping, "Loop")),
            Button::MetalMetal => Some((app.metal_metal, METAL_METAL_LABEL)),
            Button::Smooth => Some((app.smooth, "Smooth")),
            Button::RelativeEnergy => Some((app.relative_energy, RELATIVE_ENERGY_LABEL)),
            Button::FixOrientation => {
                Some((app.fixed_orientation.is_some(), FIX_ORIENTATION_LABEL))
            }
            Button::Light => Some((app.light, LIGHT_LABEL)),
            Button::Transparent => Some((app.transparent, TRANSPARENT_LABEL)),
            Button::Forces => Some((app.forces, FORCES_LABEL)),
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
            c.rect(d, bx, if on { ACCENT } else { th.unchecked })?;
            c.outline(d, bx, if hovered { th.text } else { th.weak })?;
            if on {
                let (x0, y0) = (bx.x + 3, bx.y + s / 2);
                let tick = [
                    seg(x0, y0, x0 + s / 4, bx.y + s - 4),
                    seg(x0 + s / 4, bx.y + s - 4, bx.x + s - 3, bx.y + 3),
                ];
                c.segments(d, &tick, 2, [1.0, 1.0, 1.0])?;
            }
            return c.text(d, bx.x + s + cw / 2 + 2, ty, label, th.text);
        }
        let face = if hovered { th.face_hover } else { th.face };
        c.rect(d, r, face)?;
        c.outline(d, r, th.border)?;
        let label = playback_label(b, app.player.playing);
        let cx = match label {
            Some(text) => {
                c.text(d, r.x + r.h + cw / 2, ty, text, th.text)?;
                r.x + r.h / 2 + 2
            }
            None => r.x + r.w / 2,
        };
        let cy = r.y + r.h / 2;
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
                c.rect(d, bar(cx - k - 3), th.text)?;
                c.polygon(d, &left(cx + 1), th.text)?;
            }
            Button::Prev => c.polygon(d, &left(cx), th.text)?,
            Button::Play if app.player.playing => {
                c.rect(
                    d,
                    Rect {
                        x: cx - k + 1,
                        y: cy - k,
                        w: k - 1,
                        h: 2 * k + 1,
                    },
                    th.text,
                )?;
                c.rect(
                    d,
                    Rect {
                        x: cx + 1,
                        y: cy - k,
                        w: k - 1,
                        h: 2 * k + 1,
                    },
                    th.text,
                )?;
            }
            Button::Play => c.polygon(d, &right(cx + 1), ACCENT)?,
            Button::Next => c.polygon(d, &right(cx), th.text)?,
            Button::Last => {
                c.polygon(d, &right(cx - 1), th.text)?;
                c.rect(d, bar(cx + k + 2), th.text)?;
            }
            Button::Slower | Button::LessTolerance | Button::LessIso => c.rect(
                d,
                Rect {
                    x: cx - k,
                    y: cy,
                    w: 2 * k + 1,
                    h: 2,
                },
                th.text,
            )?,
            Button::Faster | Button::MoreTolerance | Button::MoreIso => {
                c.rect(
                    d,
                    Rect {
                        x: cx - k,
                        y: cy,
                        w: 2 * k + 1,
                        h: 2,
                    },
                    th.text,
                )?;
                c.rect(
                    d,
                    Rect {
                        x: cx,
                        y: cy - k,
                        w: 2,
                        h: 2 * k + 1,
                    },
                    th.text,
                )?;
            }
            Button::Loop
            | Button::MetalMetal
            | Button::Smooth
            | Button::RelativeEnergy
            | Button::FixOrientation
            | Button::Light
            | Button::Transparent
            | Button::Forces => {}
        }
        Ok(())
    }

    fn tooltip(&self, app: &App, x: i32, y: i32, lines: &[String]) -> Res<()> {
        let th = theme(app);
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
        c.rect(self.back, r, th.tooltip)?;
        c.outline(self.back, r, th.weak)?;
        c.set_font(self.font.id)?;
        for (k, t) in lines.iter().enumerate() {
            c.text(self.back, bx + 6, by + 4 + asc + k as i32 * lh, t, th.text)?;
        }
        Ok(())
    }
}

fn seg(x1: i32, y1: i32, x2: i32, y2: i32) -> Segment {
    let c = |v: i32| v.clamp(-32000, 32000) as i16;
    Segment {
        x1: c(x1),
        y1: c(y1),
        x2: c(x2),
        y2: c(y2),
    }
}

fn clip(a: (f64, f64), b: (f64, f64)) -> Option<((f64, f64), (f64, f64))> {
    const L: f64 = 30000.0;
    if ![a.0, a.1, b.0, b.1].iter().all(|v| v.is_finite()) {
        return None;
    }
    let d = (b.0 - a.0, b.1 - a.1);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [
        (-d.0, a.0 + L),
        (d.0, L - a.0),
        (-d.1, a.1 + L),
        (d.1, L - a.1),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else if p < 0.0 {
            t0 = t0.max(q / p);
        } else {
            t1 = t1.min(q / p);
        }
    }
    (t0 <= t1).then_some((
        (a.0 + t0 * d.0, a.1 + t0 * d.1),
        (a.0 + t1 * d.0, a.1 + t1 * d.1),
    ))
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

pub fn run(loaded: Loaded, title: &str, options: Options) -> Result<(), String> {
    run_inner(loaded, title, options).map_err(|e| e.to_string())
}

fn connect() -> Result<(RustConnection, usize), Box<dyn std::error::Error>> {
    #[cfg(unix)]
    if let Some(path) = std::env::var("DISPLAY")
        .ok()
        .filter(|d| d.starts_with('/') && std::path::Path::new(d).exists())
    {
        use x11rb::reexports::x11rb_protocol::xauth::get_auth;
        use x11rb::rust_connection::DefaultStream;
        let stream = std::os::unix::net::UnixStream::connect(path)?;
        let (stream, (family, address)) = DefaultStream::from_unix_stream(stream)?;
        let (name, data) = get_auth(family, &address, 0)
            .ok()
            .flatten()
            .unwrap_or_default();
        return Ok((
            RustConnection::connect_to_stream_with_auth_info(stream, 0, name, data)?,
            0,
        ));
    }
    Ok(x11rb::connect(None)?)
}

fn run_inner(loaded: Loaded, title: &str, options: Options) -> Res<()> {
    let (conn, screen_num) =
        connect().map_err(|e| format!("cannot connect to the X display: {e}"))?;
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
        r.layer = conn.generate_id()?;
        r.layer_picture = conn.generate_id()?;
        r.layer_mask = conn.generate_id()?;
        let alpha = (SEE_THROUGH * 65535.0) as u16;
        let color = render::Color {
            red: 0,
            green: 0,
            blue: 0,
            alpha,
        };
        conn.render_create_solid_fill(r.layer_mask, color)?;
    }
    app.configure(&options);
    if app.smooth && render.is_none() {
        eprintln!("mole: warning: the X server has no XRender >= 0.10; using basic drawing");
        app.smooth = false;
        app.transparent = false;
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
    const ICON: &[u8] = include_bytes!("../assets/icon.argb");
    let icon: Vec<u32> = ICON
        .as_chunks()
        .0
        .iter()
        .map(|&b| u32::from_le_bytes(b))
        .collect();
    conn.change_property32(
        PropMode::REPLACE,
        win,
        atom(b"_NET_WM_ICON")?,
        AtomEnum::CARDINAL,
        &icon,
    )?;
    conn.change_property32(
        PropMode::REPLACE,
        win,
        wm_protocols,
        AtomEnum::ATOM,
        &[wm_delete],
    )?;
    let (min_w, min_h) = Layout::min_size(font.char_w, font.line_h, app.layout.panes);
    let mut hints = [0u32; 18];
    hints[0] = 16;
    hints[5] = min_w as u32;
    hints[6] = min_h as u32;
    conn.change_property32(
        PropMode::REPLACE,
        win,
        AtomEnum::WM_NORMAL_HINTS,
        AtomEnum::WM_SIZE_HINTS,
        &hints,
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
    if let Some((dx, dy)) = std::env::var("MOLE_CAPTURE_DRAG").ok().and_then(|s| {
        let (a, b) = s.split_once(',')?;
        Some((a.parse::<i32>().ok()?, b.parse::<i32>().ok()?))
    }) {
        let v = app.layout.view;
        let (x, y) = (v.x + v.w / 2, v.y + v.h / 2);
        app.handle(Input::Motion { x, y }, 0.0);
        app.handle(Input::Press { x, y, button: 1 }, 0.0);
        app.handle(
            Input::Motion {
                x: x + dx,
                y: y + dy,
            },
            0.0,
        );
        app.handle(Input::Release { button: 1 }, 0.0);
    }

    let mut x_errors = 0u32;
    let t0 = Instant::now();
    let now = || t0.elapsed().as_secs_f64();
    let cursor_font = conn.generate_id()?;
    conn.open_font(cursor_font, b"cursor")?;
    let make_cursor = |glyph: u16| -> Res<u32> {
        let id = conn.generate_id()?;
        conn.create_glyph_cursor(
            id,
            cursor_font,
            cursor_font,
            glyph,
            glyph + 1,
            0,
            0,
            0,
            0xffff,
            0xffff,
            0xffff,
        )?;
        Ok(id)
    };
    let (resize_v, resize_h) = (make_cursor(116)?, make_cursor(108)?);
    let edge_of = |h: Hover| match h {
        Hover::Edge(e) => Some(e),
        _ => None,
    };
    conn.close_font(cursor_font)?;
    let (mut dirty, mut plot_dirty) = (true, true);
    let mut pending_sync = None;
    let mut frames = 0usize;
    let mut bench_start = 0.0;
    loop {
        let before = app.player.current;
        let wait = app.player.tick(now());
        if app.player.current != before {
            app.refresh_hover();
            dirty = true;
        }
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
                    Some(Input::Resize {
                        w: c.width.into(),
                        h: c.height.into(),
                    })
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
                Event::Error(e) => {
                    x_errors += 1;
                    if x_errors <= 3 {
                        eprintln!(
                            "mole: X error {} (request {})",
                            e.error_code, e.major_opcode
                        );
                    }
                    None
                }
                _ => None,
            };
            if let Some(input) = input {
                let plot_look = |a: &App| (a.relative_energy, a.light);
                let old_look = plot_look(&app);
                let size = |l: &Layout| (l.w, l.h, l.plot);
                let old_size = size(&app.layout);
                let old_edge = edge_of(app.hover);
                dirty |= app.handle(input, now());
                plot_dirty |= plot_look(&app) != old_look;
                if size(&app.layout) != old_size {
                    window.free_pixmaps()?;
                    window.make_pixmaps(&app.layout)?;
                    plot_dirty = true;
                }
                let edge = edge_of(app.hover);
                if edge != old_edge {
                    let cursor = match edge {
                        Some(Edge::Plot) => resize_v,
                        Some(Edge::Panel) => resize_h,
                        None => x11rb::NONE,
                    };
                    let aux = ChangeWindowAttributesAux::new().cursor(cursor);
                    conn.change_window_attributes(win, &aux)?;
                }
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
    use super::{clip, latin1};

    #[test]
    fn far_lines_keep_their_slope() {
        assert_eq!(
            clip((0.0, 0.0), (10.0, 5.0)),
            Some(((0.0, 0.0), (10.0, 5.0)))
        );
        let ((x0, y0), (x1, y1)) = clip((-90000.0, -45000.0), (90000.0, 45000.0)).unwrap();
        assert!((y0 / x0 - 0.5).abs() < 1e-12 && (y1 / x1 - 0.5).abs() < 1e-12);
        assert!(x0 >= -30000.0 && x1 <= 30000.0);
        assert_eq!(clip((40000.0, 0.0), (50000.0, 10.0)), None);
        assert_eq!(clip((f64::NAN, 0.0), (1.0, 1.0)), None);
    }

    #[test]
    fn latin1_replaces_what_server_fonts_cannot_show() {
        assert_eq!(latin1("Å·x"), vec![0xC5, 0xB7, b'x']);
        assert_eq!(latin1("a–b"), b"a?b".to_vec());
    }
}
