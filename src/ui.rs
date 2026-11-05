//! The popup panel: a tiny immediate-style UI drawn in software
//! (tiny-skia shapes + fontdue text). Everything here is dropped while the
//! panel is hidden.

use std::collections::HashMap;

use tiny_skia::{FillRule, Paint, Path, PathBuilder, Pixmap, Transform};

use crate::settings::{BEAT_RANGE, CARRIER_RANGE, NOISE_NAMES, PRESETS, Settings};

pub const W: f32 = 300.0;
pub const H: f32 = 474.0;

type Rgb = (u8, u8, u8);
const BG: Rgb = (20, 22, 28);
const CARD: Rgb = (31, 34, 42);
const TRACK: Rgb = (52, 56, 68);
const TEXT: Rgb = (234, 236, 242);
const DIM: Rgb = (132, 138, 154);
const NOISE_COLORS: [Rgb; 3] = [(226, 230, 240), (255, 121, 176), (214, 146, 92)];
const BIN_COLOR: Rgb = (122, 162, 255);
const MASTER_COLOR: Rgb = (110, 220, 160);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SliderId {
    Noise(usize),
    BinVol,
    Beat,
    Carrier,
    Master,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Ctrl {
    Play,
    NoiseToggle(usize),
    BinToggle,
    Preset(usize),
    Slider(SliderId),
}

#[derive(Clone, Copy)]
struct R {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl R {
    fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

const fn r(x: f32, y: f32, w: f32, h: f32) -> R {
    R { x, y, w, h }
}

// Layout (logical px).
const NOISE_ROW_Y: [f32; 3] = [114.0, 148.0, 182.0];
const BIN_ROW_Y: f32 = 248.0;
const PRESET_Y: f32 = 270.0;
const BEAT_Y: f32 = 336.0;
const CARRIER_Y: f32 = 368.0;
const MASTER_Y: f32 = 428.0;
const TOGGLE_X: f32 = 24.0;
const LABEL_X: f32 = 66.0;
const RIGHT: f32 = 278.0;

fn slider_track(id: SliderId) -> R {
    let (x0, y) = match id {
        SliderId::Noise(i) => (118.0, NOISE_ROW_Y[i]),
        SliderId::BinVol => (118.0, BIN_ROW_Y),
        SliderId::Beat => (70.0, BEAT_Y),
        SliderId::Carrier => (70.0, CARRIER_Y),
        SliderId::Master => (84.0, MASTER_Y),
    };
    let x1 = match id {
        SliderId::Beat | SliderId::Carrier => 214.0,
        _ => 238.0,
    };
    r(x0, y - 3.0, x1 - x0, 6.0)
}

fn preset_rect(i: usize) -> R {
    let w = (RIGHT - 22.0 - 3.0 * 6.0) / 4.0;
    r(22.0 + i as f32 * (w + 6.0), PRESET_Y, w, 42.0)
}

fn hit(x: f32, y: f32) -> Option<Ctrl> {
    if (x - 40.0).powi(2) + (y - 36.0).powi(2) <= 24.0f32.powi(2) {
        return Some(Ctrl::Play);
    }
    let sliders = [
        SliderId::Noise(0),
        SliderId::Noise(1),
        SliderId::Noise(2),
        SliderId::BinVol,
        SliderId::Beat,
        SliderId::Carrier,
        SliderId::Master,
    ];
    for id in sliders {
        let t = slider_track(id);
        if r(t.x - 10.0, t.y - 12.0, t.w + 20.0, t.h + 24.0).contains(x, y) {
            return Some(Ctrl::Slider(id));
        }
    }
    for (i, cy) in NOISE_ROW_Y.iter().enumerate() {
        if r(TOGGLE_X - 6.0, cy - 15.0, 110.0 - TOGGLE_X, 30.0).contains(x, y) {
            return Some(Ctrl::NoiseToggle(i));
        }
    }
    if r(TOGGLE_X - 6.0, BIN_ROW_Y - 15.0, 110.0 - TOGGLE_X, 30.0).contains(x, y) {
        return Some(Ctrl::BinToggle);
    }
    (0..PRESETS.len()).find(|&i| preset_rect(i).contains(x, y)).map(Ctrl::Preset)
}

fn log_norm(v: f32, (lo, hi): (f32, f32)) -> f32 {
    (v / lo).ln() / (hi / lo).ln()
}

fn log_denorm(t: f32, (lo, hi): (f32, f32)) -> f32 {
    lo * (hi / lo).powf(t)
}

fn get_norm(s: &Settings, id: SliderId) -> f32 {
    match id {
        SliderId::Noise(i) => s.noise_vol[i],
        SliderId::BinVol => s.bin_vol,
        SliderId::Beat => log_norm(s.beat, BEAT_RANGE),
        SliderId::Carrier => log_norm(s.carrier, CARRIER_RANGE),
        SliderId::Master => s.master,
    }
}

fn set_norm(s: &mut Settings, id: SliderId, t: f32) {
    let t = t.clamp(0.0, 1.0);
    match id {
        SliderId::Noise(i) => {
            s.noise_vol[i] = t;
            s.noise_on[i] = true;
        }
        SliderId::BinVol => {
            s.bin_vol = t;
            s.bin_on = true;
        }
        // Round to friendly steps so presets and labels line up.
        SliderId::Beat => s.beat = (log_denorm(t, BEAT_RANGE) * 10.0).round() / 10.0,
        SliderId::Carrier => s.carrier = log_denorm(t, CARRIER_RANGE).round(),
        SliderId::Master => s.master = t,
    }
}

/// What the app should do after an input event.
#[derive(PartialEq, Debug)]
pub enum Response {
    None,
    Changed,
    TogglePlay,
}

#[derive(Default)]
pub struct Ui {
    drag: Option<SliderId>,
    hover: Option<Ctrl>,
}

impl Ui {
    pub fn press(&mut self, s: &mut Settings, x: f32, y: f32) -> Response {
        match hit(x, y) {
            None => Response::None,
            Some(Ctrl::Play) => Response::TogglePlay,
            Some(Ctrl::NoiseToggle(i)) => {
                s.noise_on[i] = !s.noise_on[i];
                Response::Changed
            }
            Some(Ctrl::BinToggle) => {
                s.bin_on = !s.bin_on;
                Response::Changed
            }
            Some(Ctrl::Preset(i)) => {
                s.beat = PRESETS[i].2;
                s.bin_on = true;
                Response::Changed
            }
            Some(Ctrl::Slider(id)) => {
                self.drag = Some(id);
                self.drag_to(s, x)
            }
        }
    }

    pub fn release(&mut self) {
        self.drag = None;
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    pub fn moved(&mut self, s: &mut Settings, x: f32, y: f32) -> Response {
        if self.drag.is_some() {
            return self.drag_to(s, x);
        }
        let h = hit(x, y);
        if h != self.hover {
            self.hover = h;
            return Response::Changed;
        }
        Response::None
    }

    pub fn wheel(&mut self, s: &mut Settings, x: f32, y: f32, notches: f32) -> Response {
        let Some(Ctrl::Slider(id)) = hit(x, y) else { return Response::None };
        let t = get_norm(s, id) + notches * 0.02;
        set_norm(s, id, t);
        Response::Changed
    }

    fn drag_to(&mut self, s: &mut Settings, x: f32) -> Response {
        let Some(id) = self.drag else { return Response::None };
        let t = slider_track(id);
        set_norm(s, id, (x - t.x) / t.w);
        Response::Changed
    }

    pub fn draw(&self, p: &mut Painter, s: &Settings, playing: bool, hint: &str) {
        p.pix.fill(tiny_skia::Color::from_rgba8(BG.0, BG.1, BG.2, 255));

        // Header: play button + title.
        let hov = self.hover == Some(Ctrl::Play);
        let btn = if playing { MASTER_COLOR } else if hov { (70, 76, 92) } else { TRACK };
        p.circle(40.0, 36.0, 22.0, btn, 255);
        let ic = if playing { BG } else { TEXT };
        if playing {
            p.rrect(32.0, 27.0, 6.0, 18.0, 1.5, ic, 255);
            p.rrect(42.0, 27.0, 6.0, 18.0, 1.5, ic, 255);
        } else {
            let mut pb = PathBuilder::new();
            pb.move_to(34.0, 25.0);
            pb.line_to(51.0, 36.0);
            pb.line_to(34.0, 47.0);
            pb.close();
            p.fill(pb.finish(), ic, 255);
        }
        p.text(74.0, 32.0, 22.0, TEXT, 255, "Lull", Align::Left);
        let status = if playing { "Playing" } else { "Stopped" };
        p.text(75.0, 52.0, 12.0, if playing { MASTER_COLOR } else { DIM }, 255, status, Align::Left);
        self.draw_level_meter(p, s, playing);

        // Noise card.
        p.rrect(10.0, 72.0, W - 20.0, 132.0, 12.0, CARD, 255);
        p.text(24.0, 92.0, 10.0, DIM, 255, "NOISE", Align::Left);
        for i in 0..3 {
            let cy = NOISE_ROW_Y[i];
            let c = NOISE_COLORS[i];
            self.toggle(p, TOGGLE_X, cy, s.noise_on[i], c, Ctrl::NoiseToggle(i));
            let a = if s.noise_on[i] { 255 } else { 130 };
            p.text(LABEL_X, cy + 5.0, 13.0, TEXT, a, NOISE_NAMES[i], Align::Left);
            self.slider(p, SliderId::Noise(i), s, c, s.noise_on[i]);
            let pct = format!("{:.0}", s.noise_vol[i] * 100.0);
            p.text(RIGHT, cy + 4.0, 11.0, DIM, a, &pct, Align::Right);
        }

        // Binaural card.
        p.rrect(10.0, 212.0, W - 20.0, 178.0, 12.0, CARD, 255);
        p.text(24.0, 232.0, 10.0, DIM, 255, "BINAURAL BEATS  ·  headphones", Align::Left);
        let on = s.bin_on;
        let a = if on { 255 } else { 130 };
        self.toggle(p, TOGGLE_X, BIN_ROW_Y, on, BIN_COLOR, Ctrl::BinToggle);
        p.text(LABEL_X, BIN_ROW_Y + 5.0, 13.0, TEXT, a, "Volume", Align::Left);
        self.slider(p, SliderId::BinVol, s, BIN_COLOR, on);
        let pct = format!("{:.0}", s.bin_vol * 100.0);
        p.text(RIGHT, BIN_ROW_Y + 4.0, 11.0, DIM, a, &pct, Align::Right);

        for (i, (name, sym, hz)) in PRESETS.iter().enumerate() {
            let rc = preset_rect(i);
            let sel = (s.beat - hz).abs() < 0.05;
            let hov = self.hover == Some(Ctrl::Preset(i));
            let (fill, fa) = if sel { (BIN_COLOR, if on { 255 } else { 120 }) } else if hov { ((62, 67, 82), 255) } else { (TRACK, 255) };
            p.rrect(rc.x, rc.y, rc.w, rc.h, 9.0, fill, fa);
            let tc = if sel && on { BG } else { TEXT };
            let cx = rc.x + rc.w / 2.0;
            p.text(cx, rc.y + 19.0, 13.0, tc, 255, &format!("{sym} {name}"), Align::Center);
            p.text(cx, rc.y + 34.0, 10.0, if sel && on { BG } else { DIM }, 255, &format!("{hz:.0} Hz"), Align::Center);
        }

        p.text(24.0, BEAT_Y + 5.0, 12.0, TEXT, a, "Beat", Align::Left);
        self.slider(p, SliderId::Beat, s, BIN_COLOR, on);
        p.text(RIGHT, BEAT_Y + 5.0, 12.0, TEXT, a, &format!("{:.1} Hz", s.beat), Align::Right);
        p.text(24.0, CARRIER_Y + 5.0, 12.0, TEXT, a, "Tone", Align::Left);
        self.slider(p, SliderId::Carrier, s, BIN_COLOR, on);
        p.text(RIGHT, CARRIER_Y + 5.0, 12.0, TEXT, a, &format!("{:.0} Hz", s.carrier), Align::Right);

        // Master card.
        p.rrect(10.0, 398.0, W - 20.0, 52.0, 12.0, CARD, 255);
        p.text(24.0, MASTER_Y + 5.0, 13.0, TEXT, 255, "Master", Align::Left);
        self.slider(p, SliderId::Master, s, MASTER_COLOR, true);
        let pct = format!("{:.0}", s.master * 100.0);
        p.text(RIGHT, MASTER_Y + 4.0, 11.0, DIM, 255, &pct, Align::Right);

        p.text(W / 2.0, H - 8.0, 10.0, DIM, 255, hint, Align::Center);
    }

    /// Small coloured bars in the header showing which layers are active.
    fn draw_level_meter(&self, p: &mut Painter, s: &Settings, playing: bool) {
        let layers = [
            (s.noise_on[0], s.noise_vol[0], NOISE_COLORS[0]),
            (s.noise_on[1], s.noise_vol[1], NOISE_COLORS[1]),
            (s.noise_on[2], s.noise_vol[2], NOISE_COLORS[2]),
            (s.bin_on, s.bin_vol, BIN_COLOR),
        ];
        for (i, (on, v, c)) in layers.into_iter().enumerate() {
            let h = 6.0 + 28.0 * if on { v * s.master } else { 0.0 };
            let x = RIGHT - 4.0 * 10.0 + i as f32 * 10.0 + 4.0;
            let a = if playing && on { 255 } else if on { 120 } else { 60 };
            p.rrect(x, 52.0 - h, 6.0, h, 3.0, c, a);
        }
    }

    fn toggle(&self, p: &mut Painter, x: f32, cy: f32, on: bool, c: Rgb, me: Ctrl) {
        let hov = self.hover == Some(me);
        let fill = if on { c } else if hov { (70, 76, 92) } else { TRACK };
        p.rrect(x, cy - 10.0, 34.0, 20.0, 10.0, fill, 255);
        let kx = if on { x + 24.0 } else { x + 10.0 };
        p.circle(kx, cy, 7.0, if on { BG } else { DIM }, 255);
    }

    fn slider(&self, p: &mut Painter, id: SliderId, s: &Settings, c: Rgb, on: bool) {
        let t = slider_track(id);
        let v = get_norm(s, id).clamp(0.0, 1.0);
        let a = if on { 255 } else { 110 };
        p.rrect(t.x, t.y, t.w, t.h, 3.0, TRACK, 255);
        p.rrect(t.x, t.y, (t.w * v).max(t.h), t.h, 3.0, c, a);
        let active = self.drag == Some(id) || self.hover == Some(Ctrl::Slider(id));
        let kr = if active { 8.0 } else { 7.0 };
        p.circle(t.x + t.w * v, t.y + t.h / 2.0, kr, if on { TEXT } else { DIM }, 255);
    }
}

#[derive(Clone, Copy)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Software canvas at physical resolution; draws in logical coordinates.
pub struct Painter {
    pub pix: Pixmap,
    scale: f32,
    font: fontdue::Font,
    glyphs: HashMap<(char, u16), (fontdue::Metrics, Vec<u8>)>,
}

impl Painter {
    pub fn new(scale: f32) -> Self {
        let font = fontdue::Font::from_bytes(epaint_default_fonts::UBUNTU_LIGHT, Default::default())
            .expect("embedded font");
        let pix = Pixmap::new(1, 1).unwrap();
        Painter { pix, scale, font, glyphs: HashMap::new() }
    }

    pub fn resize(&mut self, w: u32, h: u32, scale: f32) {
        if self.pix.width() != w || self.pix.height() != h {
            self.pix = Pixmap::new(w.max(1), h.max(1)).unwrap();
        }
        if self.scale != scale {
            self.scale = scale;
            self.glyphs.clear();
        }
    }

    fn paint(c: Rgb, a: u8) -> Paint<'static> {
        let mut p = Paint::default();
        p.set_color_rgba8(c.0, c.1, c.2, a);
        p.anti_alias = true;
        p
    }

    fn fill(&mut self, path: Option<Path>, c: Rgb, a: u8) {
        if let Some(path) = path {
            let t = Transform::from_scale(self.scale, self.scale);
            self.pix.fill_path(&path, &Self::paint(c, a), FillRule::Winding, t, None);
        }
    }

    fn circle(&mut self, cx: f32, cy: f32, rad: f32, c: Rgb, a: u8) {
        self.fill(PathBuilder::from_circle(cx, cy, rad), c, a);
    }

    fn rrect(&mut self, x: f32, y: f32, w: f32, h: f32, rad: f32, c: Rgb, a: u8) {
        let rad = rad.min(w / 2.0).min(h / 2.0);
        let (x1, y1) = (x + w, y + h);
        let mut pb = PathBuilder::new();
        pb.move_to(x + rad, y);
        pb.line_to(x1 - rad, y);
        pb.quad_to(x1, y, x1, y + rad);
        pb.line_to(x1, y1 - rad);
        pb.quad_to(x1, y1, x1 - rad, y1);
        pb.line_to(x + rad, y1);
        pb.quad_to(x, y1, x, y1 - rad);
        pb.line_to(x, y + rad);
        pb.quad_to(x, y, x + rad, y);
        pb.close();
        self.fill(pb.finish(), c, a);
    }

    /// Draws text with its baseline at `y` (logical px).
    fn text(&mut self, x: f32, y: f32, size: f32, c: Rgb, a: u8, s: &str, align: Align) {
        let px = (size * self.scale).round().max(1.0) as u16;
        let width: f32 = s.chars().map(|ch| self.font.metrics(ch, px as f32).advance_width).sum();
        let mut pen = x * self.scale
            - match align {
                Align::Left => 0.0,
                Align::Center => width / 2.0,
                Align::Right => width,
            };
        let base = (y * self.scale).round() as i32;
        let (pw, ph) = (self.pix.width() as i32, self.pix.height() as i32);
        for ch in s.chars() {
            let font = &self.font;
            let (m, bmp) = self
                .glyphs
                .entry((ch, px))
                .or_insert_with(|| font.rasterize(ch, px as f32));
            let gx = pen.round() as i32 + m.xmin;
            let gy = base - m.ymin - m.height as i32;
            let data = self.pix.data_mut();
            for row in 0..m.height as i32 {
                let yy = gy + row;
                if yy < 0 || yy >= ph {
                    continue;
                }
                for col in 0..m.width as i32 {
                    let xx = gx + col;
                    if xx < 0 || xx >= pw {
                        continue;
                    }
                    let cov = bmp[(row * m.width as i32 + col) as usize];
                    if cov == 0 {
                        continue;
                    }
                    // Slight gamma boost keeps the light font readable on dark.
                    let k = (cov as f32 / 255.0).powf(0.7) * a as f32 / 255.0;
                    let i = ((yy * pw + xx) * 4) as usize;
                    for (j, src) in [c.0, c.1, c.2].into_iter().enumerate() {
                        let d = data[i + j] as f32;
                        data[i + j] = (d + (src as f32 - d) * k) as u8;
                    }
                }
            }
            pen += m.advance_width;
        }
    }

    /// Copies the canvas into a softbuffer `0RGB` frame.
    pub fn blit(&self, out: &mut [u32]) {
        for (o, px) in out.iter_mut().zip(self.pix.data().chunks_exact(4)) {
            *o = (px[0] as u32) << 16 | (px[1] as u32) << 8 | px[2] as u32;
        }
    }
}

/// Renders the tray icon (three wave bars) as RGBA.
pub fn tray_icon_rgba(size: u32, playing: bool, template: bool) -> Vec<u8> {
    let mut pix = Pixmap::new(size, size).unwrap();
    let s = size as f32 / 32.0;
    let c = if template { (0, 0, 0) } else { (255, 255, 255) };
    let a = if playing { 255 } else { 150 };
    let mut paint = Painter::paint(c, a);
    paint.anti_alias = true;
    for (i, h) in [14.0f32, 24.0, 18.0].into_iter().enumerate() {
        let x = 5.0 + i as f32 * 8.5;
        let rect = tiny_skia::Rect::from_xywh(x * s, (16.0 - h / 2.0) * s, 5.0 * s, h * s).unwrap();
        pix.fill_rect(rect, &paint, Transform::identity(), None);
    }
    // tiny-skia stores premultiplied colour; tray icons expect straight alpha.
    pix.pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_slider_roundtrip() {
        for v in [0.5, 2.0, 10.0, 40.0] {
            let t = log_norm(v, BEAT_RANGE);
            assert!((log_denorm(t, BEAT_RANGE) - v).abs() < 1e-3);
        }
    }

    #[test]
    fn preset_click_sets_beat() {
        let mut s = Settings::default();
        let rc = preset_rect(1);
        let mut ui = Ui::default();
        assert_eq!(ui.press(&mut s, rc.x + 5.0, rc.y + 5.0), Response::Changed);
        assert_eq!(s.beat, 6.0);
        assert!(s.bin_on);
    }

    #[test]
    fn draws_without_panic() {
        let mut p = Painter::new(1.5);
        p.resize((W * 1.5) as u32, (H * 1.5) as u32, 1.5);
        Ui::default().draw(&mut p, &Settings::default(), true, "hint");
        let mut out = vec![0u32; (p.pix.width() * p.pix.height()) as usize];
        p.blit(&mut out);
        assert!(out.iter().any(|&c| c != out[0]));
    }

    /// `LULL_PREVIEW=out.png cargo test preview -- --ignored` renders the panel.
    #[test]
    #[ignore]
    fn preview() {
        let path = std::env::var("LULL_PREVIEW").unwrap_or("preview.png".into());
        let mut p = Painter::new(2.0);
        p.resize((W * 2.0) as u32, (H * 2.0) as u32, 2.0);
        let mut s = Settings::default();
        s.bin_on = true;
        s.noise_on[2] = true;
        Ui::default().draw(&mut p, &s, true, "Ctrl+Alt+N  play / stop      Ctrl+Alt+M  panel");
        p.pix.save_png(path).unwrap();
    }
}
