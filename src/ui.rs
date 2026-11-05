//! The popup panel and the on-screen timer: a tiny UI drawn in software
//! (tiny-skia shapes + fontdue text). Everything here is dropped while the
//! windows are hidden.

use std::collections::HashMap;
use std::time::Duration;

use tiny_skia::{FillRule, Paint, Path, PathBuilder, Pixmap, Transform};

use crate::pomodoro::{Phase, clock};
use crate::settings::{
    BEAT_RANGE, BREAK_RANGE, CARRIER_RANGE, FOCUS_RANGE, NOISE_NAMES, OPACITY_RANGE,
    OVERLAY_SIZE_RANGE, POMO_PRESETS, PRESETS, ROUNDS_RANGE, Settings,
};

pub const W: f32 = 300.0;
pub const H: f32 = 530.0;
/// On-screen timer size at 100 %.
pub const OVERLAY_W: f32 = 168.0;
pub const OVERLAY_H: f32 = 60.0;

type Rgb = (u8, u8, u8);
const BG: Rgb = (20, 22, 28);
const CARD: Rgb = (31, 34, 42);
const TRACK: Rgb = (52, 56, 68);
const HOVER: Rgb = (66, 72, 88);
const TEXT: Rgb = (234, 236, 242);
const DIM: Rgb = (132, 138, 154);
const NOISE_COLORS: [Rgb; 3] = [(226, 230, 240), (255, 121, 176), (214, 146, 92)];
const BIN_COLOR: Rgb = (122, 162, 255);
const MASTER_COLOR: Rgb = (110, 220, 160);
const TIMER_COLOR: Rgb = (255, 140, 105);

pub fn phase_color(p: Phase) -> Rgb {
    match p {
        Phase::Focus => TIMER_COLOR,
        Phase::Break => MASTER_COLOR,
        Phase::LongBreak => BIN_COLOR,
    }
}

pub fn phase_name(p: Phase) -> &'static str {
    match p {
        Phase::Focus => "Focus",
        Phase::Break => "Break",
        Phase::LongBreak => "Long break",
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Tab {
    #[default]
    Sound,
    Timer,
}

/// Snapshot of the timer for drawing.
#[derive(Clone, Copy, Debug)]
pub struct TimerView {
    pub active: bool,
    pub running: bool,
    pub phase: Phase,
    pub round: u32,
    pub rounds: u32,
    pub remaining: Duration,
    pub progress: f32,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SliderId {
    Noise(usize),
    BinVol,
    Beat,
    Carrier,
    Master,
    Focus,
    Break,
    Long,
    Rounds,
    Opacity,
    Size,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Tog {
    Noise(usize),
    Bin,
    SoundOnFocus,
    SoundOffAtEnd,
    Chime,
    Overlay,
    OnTop,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Ctrl {
    Play,
    Tab(Tab),
    Toggle(Tog),
    Preset(usize),
    PomoPreset(usize),
    Slider(SliderId),
    Timer(TimerCmd),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimerCmd {
    StartPause,
    Skip,
    Reset,
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

// Layout (logical px). Header 0..64, tabs 70..98, content from 106.
const PLAY: (f32, f32, f32) = (40.0, 36.0, 22.0);
const TAB_Y: f32 = 70.0;
const RIGHT: f32 = 278.0;
// Sound tab.
const NOISE_ROW_Y: [f32; 3] = [148.0, 182.0, 216.0];
const BIN_ROW_Y: f32 = 282.0;
const PRESET_Y: f32 = 304.0;
const BEAT_Y: f32 = 370.0;
const CARRIER_Y: f32 = 402.0;
const MASTER_Y: f32 = 462.0;
const TOGGLE_X: f32 = 24.0;
const LABEL_X: f32 = 66.0;
// Timer tab.
const T_MAIN: (f32, f32, f32) = (194.0, 146.0, 21.0);
const T_SKIP: (f32, f32, f32) = (234.0, 146.0, 15.0);
const T_RESET: (f32, f32, f32) = (268.0, 146.0, 15.0);
const POMO_PRESET_Y: f32 = 222.0;
const FOCUS_Y: f32 = 270.0;
const BREAK_Y: f32 = 294.0;
const LONG_Y: f32 = 318.0;
const ROUNDS_Y: f32 = 342.0;
const OPT_ROWS: [f32; 5] = [398.0, 422.0, 446.0, 470.0, 494.0];
const OPT_COLS: [f32; 3] = [24.0, 112.0, 200.0];

fn slider_track(id: SliderId) -> R {
    let (x0, x1, y) = match id {
        SliderId::Noise(i) => (118.0, 238.0, NOISE_ROW_Y[i]),
        SliderId::BinVol => (118.0, 238.0, BIN_ROW_Y),
        SliderId::Beat => (70.0, 214.0, BEAT_Y),
        SliderId::Carrier => (70.0, 214.0, CARRIER_Y),
        SliderId::Master => (84.0, 238.0, MASTER_Y),
        SliderId::Focus => (84.0, 222.0, FOCUS_Y),
        SliderId::Break => (84.0, 222.0, BREAK_Y),
        SliderId::Long => (84.0, 222.0, LONG_Y),
        SliderId::Rounds => (84.0, 222.0, ROUNDS_Y),
        SliderId::Opacity => (84.0, 238.0, OPT_ROWS[3]),
        SliderId::Size => (84.0, 238.0, OPT_ROWS[4]),
    };
    r(x0, y - 3.0, x1 - x0, 6.0)
}

fn preset_rect(i: usize) -> R {
    let w = (RIGHT - 22.0 - 3.0 * 6.0) / 4.0;
    r(22.0 + i as f32 * (w + 6.0), PRESET_Y, w, 42.0)
}

fn pomo_preset_rect(i: usize) -> R {
    let w = (RIGHT - 22.0 - 3.0 * 6.0) / 4.0;
    r(22.0 + i as f32 * (w + 6.0), POMO_PRESET_Y, w, 28.0)
}

fn tab_rect(t: Tab) -> R {
    let w = (W - 20.0) / 2.0;
    r(10.0 + if t == Tab::Timer { w } else { 0.0 }, TAB_Y, w, 28.0)
}

/// Toggles: (control, x, centre y, small?, label).
fn toggles(tab: Tab) -> Vec<(Tog, f32, f32, bool, &'static str)> {
    match tab {
        Tab::Sound => {
            let mut v: Vec<_> = (0..3)
                .map(|i| (Tog::Noise(i), TOGGLE_X, NOISE_ROW_Y[i], false, NOISE_NAMES[i]))
                .collect();
            v.push((Tog::Bin, TOGGLE_X, BIN_ROW_Y, false, "Volume"));
            v
        }
        Tab::Timer => vec![
            (Tog::SoundOnFocus, OPT_COLS[0], OPT_ROWS[0], true, "Play sound when focus starts"),
            (Tog::SoundOffAtEnd, OPT_COLS[0], OPT_ROWS[1], true, "Stop sound when focus ends"),
            (Tog::Chime, OPT_COLS[0], OPT_ROWS[2], true, "Chime"),
            (Tog::Overlay, OPT_COLS[1], OPT_ROWS[2], true, "On-screen"),
            (Tog::OnTop, OPT_COLS[2], OPT_ROWS[2], true, "On top"),
        ],
    }
}

fn toggle_hit(tab: Tab, t: Tog, x: f32, cy: f32) -> R {
    let w = match (tab, t) {
        (Tab::Sound, _) => 110.0 - x,
        (_, Tog::SoundOnFocus | Tog::SoundOffAtEnd) => RIGHT - x,
        _ => 86.0,
    };
    r(x - 6.0, cy - 12.0, w, 24.0)
}

fn sliders(tab: Tab) -> &'static [SliderId] {
    match tab {
        Tab::Sound => &[
            SliderId::Noise(0),
            SliderId::Noise(1),
            SliderId::Noise(2),
            SliderId::BinVol,
            SliderId::Beat,
            SliderId::Carrier,
            SliderId::Master,
        ],
        Tab::Timer => &[
            SliderId::Focus,
            SliderId::Break,
            SliderId::Long,
            SliderId::Rounds,
            SliderId::Opacity,
            SliderId::Size,
        ],
    }
}

fn in_circle(x: f32, y: f32, (cx, cy, rad): (f32, f32, f32)) -> bool {
    (x - cx).powi(2) + (y - cy).powi(2) <= (rad + 2.0).powi(2)
}

fn hit(tab: Tab, x: f32, y: f32) -> Option<Ctrl> {
    if in_circle(x, y, PLAY) {
        return Some(Ctrl::Play);
    }
    for t in [Tab::Sound, Tab::Timer] {
        if tab_rect(t).contains(x, y) {
            return Some(Ctrl::Tab(t));
        }
    }
    for &id in sliders(tab) {
        let t = slider_track(id);
        if r(t.x - 10.0, t.y - 9.0, t.w + 20.0, t.h + 18.0).contains(x, y) {
            return Some(Ctrl::Slider(id));
        }
    }
    for (t, tx, cy, _, _) in toggles(tab) {
        if toggle_hit(tab, t, tx, cy).contains(x, y) {
            return Some(Ctrl::Toggle(t));
        }
    }
    match tab {
        Tab::Sound => (0..PRESETS.len()).find(|&i| preset_rect(i).contains(x, y)).map(Ctrl::Preset),
        Tab::Timer => {
            for (c, cmd) in [(T_MAIN, TimerCmd::StartPause), (T_SKIP, TimerCmd::Skip), (T_RESET, TimerCmd::Reset)] {
                if in_circle(x, y, c) {
                    return Some(Ctrl::Timer(cmd));
                }
            }
            (0..POMO_PRESETS.len()).find(|&i| pomo_preset_rect(i).contains(x, y)).map(Ctrl::PomoPreset)
        }
    }
}

fn log_norm(v: f32, (lo, hi): (f32, f32)) -> f32 {
    (v / lo).ln() / (hi / lo).ln()
}

fn log_denorm(t: f32, (lo, hi): (f32, f32)) -> f32 {
    lo * (hi / lo).powf(t)
}

fn lin_norm(v: f32, (lo, hi): (f32, f32)) -> f32 {
    (v - lo) / (hi - lo)
}

fn lin_denorm(t: f32, (lo, hi): (f32, f32)) -> f32 {
    lo + (hi - lo) * t
}

fn get_norm(s: &Settings, id: SliderId) -> f32 {
    match id {
        SliderId::Noise(i) => s.noise_vol[i],
        SliderId::BinVol => s.bin_vol,
        SliderId::Beat => log_norm(s.beat, BEAT_RANGE),
        SliderId::Carrier => log_norm(s.carrier, CARRIER_RANGE),
        SliderId::Master => s.master,
        SliderId::Focus => log_norm(s.focus_min, FOCUS_RANGE),
        SliderId::Break => log_norm(s.break_min, BREAK_RANGE),
        SliderId::Long => log_norm(s.long_min, BREAK_RANGE),
        SliderId::Rounds => lin_norm(s.rounds, ROUNDS_RANGE),
        SliderId::Opacity => lin_norm(s.overlay_opacity, OPACITY_RANGE),
        SliderId::Size => lin_norm(s.overlay_size, OVERLAY_SIZE_RANGE),
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
        SliderId::Focus => s.focus_min = log_denorm(t, FOCUS_RANGE).round(),
        SliderId::Break => s.break_min = log_denorm(t, BREAK_RANGE).round(),
        SliderId::Long => s.long_min = log_denorm(t, BREAK_RANGE).round(),
        SliderId::Rounds => s.rounds = lin_denorm(t, ROUNDS_RANGE).round(),
        SliderId::Opacity => s.overlay_opacity = (lin_denorm(t, OPACITY_RANGE) * 100.0).round() / 100.0,
        SliderId::Size => s.overlay_size = (lin_denorm(t, OVERLAY_SIZE_RANGE) * 10.0).round() / 10.0,
    }
}

/// Mouse wheel step for a slider (in normalised units).
fn wheel_step(id: SliderId) -> f32 {
    match id {
        SliderId::Rounds => 1.0 / (ROUNDS_RANGE.1 - ROUNDS_RANGE.0),
        SliderId::Focus | SliderId::Break | SliderId::Long => 0.03,
        _ => 0.02,
    }
}

/// What the app should do after an input event.
#[derive(PartialEq, Debug)]
pub enum Response {
    None,
    /// Only the look changed (hover, tab).
    Redraw,
    /// Settings changed.
    Changed,
    TogglePlay,
    Timer(TimerCmd),
}

#[derive(Default)]
pub struct Ui {
    pub tab: Tab,
    drag: Option<SliderId>,
    hover: Option<Ctrl>,
}

impl Ui {
    pub fn new(tab: Tab) -> Self {
        Ui { tab, ..Default::default() }
    }

    pub fn press(&mut self, s: &mut Settings, x: f32, y: f32) -> Response {
        let Some(c) = hit(self.tab, x, y) else { return Response::None };
        match c {
            Ctrl::Play => Response::TogglePlay,
            Ctrl::Tab(t) => {
                self.tab = t;
                self.hover = None;
                Response::Redraw
            }
            Ctrl::Toggle(t) => {
                let b = match t {
                    Tog::Noise(i) => &mut s.noise_on[i],
                    Tog::Bin => &mut s.bin_on,
                    Tog::SoundOnFocus => &mut s.sound_on_focus,
                    Tog::SoundOffAtEnd => &mut s.sound_off_at_end,
                    Tog::Chime => &mut s.chime,
                    Tog::Overlay => &mut s.overlay,
                    Tog::OnTop => &mut s.overlay_on_top,
                };
                *b = !*b;
                Response::Changed
            }
            Ctrl::Preset(i) => {
                s.beat = PRESETS[i].2;
                s.bin_on = true;
                Response::Changed
            }
            Ctrl::PomoPreset(i) => {
                (s.focus_min, s.break_min, s.long_min, s.rounds) = POMO_PRESETS[i];
                Response::Changed
            }
            Ctrl::Slider(id) => {
                self.drag = Some(id);
                self.drag_to(s, x)
            }
            Ctrl::Timer(cmd) => Response::Timer(cmd),
        }
    }

    pub fn release(&mut self) {
        self.drag = None;
    }

    pub fn moved(&mut self, s: &mut Settings, x: f32, y: f32) -> Response {
        if self.drag.is_some() {
            return self.drag_to(s, x);
        }
        let h = hit(self.tab, x, y);
        if h != self.hover {
            self.hover = h;
            return Response::Redraw;
        }
        Response::None
    }

    pub fn wheel(&mut self, s: &mut Settings, x: f32, y: f32, notches: f32) -> Response {
        let Some(Ctrl::Slider(id)) = hit(self.tab, x, y) else { return Response::None };
        let before = get_norm(s, id);
        set_norm(s, id, before + notches.signum() * wheel_step(id).max(notches.abs() * 0.02));
        Response::Changed
    }

    fn drag_to(&mut self, s: &mut Settings, x: f32) -> Response {
        let Some(id) = self.drag else { return Response::None };
        let t = slider_track(id);
        set_norm(s, id, (x - t.x) / t.w);
        Response::Changed
    }

    pub fn draw(&self, p: &mut Painter, s: &Settings, playing: bool, timer: &TimerView, hint: &str) {
        p.clear(BG);

        // Header: play button, title, status, layer meter.
        let hov = self.hover == Some(Ctrl::Play);
        let btn = if playing { MASTER_COLOR } else if hov { HOVER } else { TRACK };
        p.circle(PLAY.0, PLAY.1, PLAY.2, btn, 255);
        play_pause_icon(p, PLAY.0, PLAY.1, 1.0, playing, if playing { BG } else { TEXT });
        p.text(74.0, 32.0, 22.0, TEXT, 255, "Lull", Align::Left);
        let status = if playing { "Playing" } else { "Stopped" };
        let sc = if playing { MASTER_COLOR } else { DIM };
        p.text(75.0, 52.0, 12.0, sc, 255, status, Align::Left);
        if timer.active {
            let x = 75.0 + p.measure(12.0, status) + 8.0;
            let mut t = format!("·  {} {}", phase_name(timer.phase), clock(timer.remaining));
            if !timer.running {
                t.push_str(" ‖");
            }
            p.text(x, 52.0, 12.0, phase_color(timer.phase), 255, &t, Align::Left);
        }
        self.level_meter(p, s, playing);

        // Tabs.
        p.rrect(10.0, TAB_Y, W - 20.0, 28.0, 9.0, CARD, 255);
        for (t, name) in [(Tab::Sound, "Sound"), (Tab::Timer, "Timer")] {
            let rc = tab_rect(t);
            let sel = self.tab == t;
            if sel {
                p.rrect(rc.x + 3.0, rc.y + 3.0, rc.w - 6.0, rc.h - 6.0, 7.0, TRACK, 255);
            } else if self.hover == Some(Ctrl::Tab(t)) {
                p.rrect(rc.x + 3.0, rc.y + 3.0, rc.w - 6.0, rc.h - 6.0, 7.0, (40, 44, 54), 255);
            }
            let c = if sel { TEXT } else { DIM };
            p.text(rc.x + rc.w / 2.0, rc.y + 19.0, 12.0, c, 255, name, Align::Center);
        }

        match self.tab {
            Tab::Sound => self.draw_sound(p, s),
            Tab::Timer => self.draw_timer(p, s, timer),
        }

        let size = (10.0f32).min(10.0 * (W - 16.0) / p.measure(10.0, hint).max(1.0));
        p.text(W / 2.0, H - 8.0, size, DIM, 255, hint, Align::Center);
    }

    fn draw_sound(&self, p: &mut Painter, s: &Settings) {
        p.rrect(10.0, 106.0, W - 20.0, 132.0, 12.0, CARD, 255);
        p.text(24.0, 126.0, 10.0, DIM, 255, "NOISE", Align::Left);
        for i in 0..3 {
            let cy = NOISE_ROW_Y[i];
            let c = NOISE_COLORS[i];
            let on = s.noise_on[i];
            self.toggle(p, Tog::Noise(i), TOGGLE_X, cy, on, c, false);
            let a = if on { 255 } else { 130 };
            p.text(LABEL_X, cy + 5.0, 13.0, TEXT, a, NOISE_NAMES[i], Align::Left);
            self.slider(p, SliderId::Noise(i), s, c, on);
            p.text(RIGHT, cy + 4.0, 11.0, DIM, a, &format!("{:.0}", s.noise_vol[i] * 100.0), Align::Right);
        }

        p.rrect(10.0, 246.0, W - 20.0, 178.0, 12.0, CARD, 255);
        p.text(24.0, 266.0, 10.0, DIM, 255, "BINAURAL BEATS  ·  headphones", Align::Left);
        let on = s.bin_on;
        let a = if on { 255 } else { 130 };
        self.toggle(p, Tog::Bin, TOGGLE_X, BIN_ROW_Y, on, BIN_COLOR, false);
        p.text(LABEL_X, BIN_ROW_Y + 5.0, 13.0, TEXT, a, "Volume", Align::Left);
        self.slider(p, SliderId::BinVol, s, BIN_COLOR, on);
        p.text(RIGHT, BIN_ROW_Y + 4.0, 11.0, DIM, a, &format!("{:.0}", s.bin_vol * 100.0), Align::Right);

        for (i, (name, sym, hz)) in PRESETS.iter().enumerate() {
            let rc = preset_rect(i);
            let sel = (s.beat - hz).abs() < 0.05;
            let lit = sel && on;
            self.chip(p, rc, sel, on, Ctrl::Preset(i), BIN_COLOR);
            let cx = rc.x + rc.w / 2.0;
            p.text(cx, rc.y + 19.0, 13.0, if lit { BG } else { TEXT }, 255, &format!("{sym} {name}"), Align::Center);
            p.text(cx, rc.y + 34.0, 10.0, if lit { BG } else { DIM }, 255, &format!("{hz:.0} Hz"), Align::Center);
        }

        p.text(24.0, BEAT_Y + 5.0, 12.0, TEXT, a, "Beat", Align::Left);
        self.slider(p, SliderId::Beat, s, BIN_COLOR, on);
        p.text(RIGHT, BEAT_Y + 5.0, 12.0, TEXT, a, &format!("{:.1} Hz", s.beat), Align::Right);
        p.text(24.0, CARRIER_Y + 5.0, 12.0, TEXT, a, "Tone", Align::Left);
        self.slider(p, SliderId::Carrier, s, BIN_COLOR, on);
        p.text(RIGHT, CARRIER_Y + 5.0, 12.0, TEXT, a, &format!("{:.0} Hz", s.carrier), Align::Right);

        p.rrect(10.0, 432.0, W - 20.0, 52.0, 12.0, CARD, 255);
        p.text(24.0, MASTER_Y + 5.0, 13.0, TEXT, 255, "Master", Align::Left);
        self.slider(p, SliderId::Master, s, MASTER_COLOR, true);
        p.text(RIGHT, MASTER_Y + 4.0, 11.0, DIM, 255, &format!("{:.0}", s.master * 100.0), Align::Right);
    }

    fn draw_timer(&self, p: &mut Painter, s: &Settings, t: &TimerView) {
        let pc = phase_color(t.phase);

        // Countdown card.
        p.rrect(10.0, 106.0, W - 20.0, 82.0, 12.0, CARD, 255);
        let label = if !t.active {
            "READY".to_string()
        } else {
            let paused = if t.running { "" } else { "  ·  PAUSED" };
            format!("{}  {}/{}{paused}", phase_name(t.phase).to_uppercase(), t.round, t.rounds)
        };
        p.circle(28.0, 122.0, 4.0, pc, if t.active { 255 } else { 90 });
        p.text(38.0, 126.0, 10.0, DIM, 255, &label, Align::Left);
        let a = if t.running || !t.active { 255 } else { 150 };
        p.text(22.0, 166.0, 38.0, TEXT, a, &clock(t.remaining), Align::Left);
        p.rrect(22.0, 177.0, RIGHT - 22.0, 3.0, 1.5, TRACK, 255);
        if t.active {
            p.rrect(22.0, 177.0, ((RIGHT - 22.0) * t.progress).max(3.0), 3.0, 1.5, pc, 255);
        }

        let main_hov = self.hover == Some(Ctrl::Timer(TimerCmd::StartPause));
        p.circle(T_MAIN.0, T_MAIN.1, T_MAIN.2, pc, if main_hov { 255 } else { 230 });
        play_pause_icon(p, T_MAIN.0, T_MAIN.1, 0.9, t.running, BG);
        for (c, cmd) in [(T_SKIP, TimerCmd::Skip), (T_RESET, TimerCmd::Reset)] {
            let hov = self.hover == Some(Ctrl::Timer(cmd));
            p.circle(c.0, c.1, c.2, if hov { HOVER } else { TRACK }, 255);
            let ic = if t.active { TEXT } else { DIM };
            if cmd == TimerCmd::Skip {
                tri(p, c.0 - 6.0, c.1, 6.0, ic);
                tri(p, c.0, c.1, 6.0, ic);
            } else {
                p.rrect(c.0 - 5.0, c.1 - 5.0, 10.0, 10.0, 2.0, ic, 255);
            }
        }

        // Schedule card.
        p.rrect(10.0, 196.0, W - 20.0, 158.0, 12.0, CARD, 255);
        p.text(24.0, 214.0, 10.0, DIM, 255, "SCHEDULE  ·  minutes", Align::Left);
        for (i, &(f, b, l, n)) in POMO_PRESETS.iter().enumerate() {
            let rc = pomo_preset_rect(i);
            let sel = s.focus_min == f && s.break_min == b && s.long_min == l && s.rounds == n;
            self.chip(p, rc, sel, true, Ctrl::PomoPreset(i), TIMER_COLOR);
            let tc = if sel { BG } else { TEXT };
            p.text(rc.x + rc.w / 2.0, rc.y + 19.0, 13.0, tc, 255, &format!("{f:.0}/{b:.0}"), Align::Center);
        }
        let rows = [
            (SliderId::Focus, FOCUS_Y, "Focus", format!("{:.0} min", s.focus_min), TIMER_COLOR),
            (SliderId::Break, BREAK_Y, "Break", format!("{:.0} min", s.break_min), MASTER_COLOR),
            (SliderId::Long, LONG_Y, "Long", format!("{:.0} min", s.long_min), BIN_COLOR),
            (SliderId::Rounds, ROUNDS_Y, "Rounds", format!("{:.0}×", s.rounds), DIM),
        ];
        for (id, y, name, val, c) in rows {
            p.text(24.0, y + 4.0, 12.0, TEXT, 255, name, Align::Left);
            self.slider(p, id, s, c, true);
            p.text(RIGHT, y + 4.0, 12.0, TEXT, 255, &val, Align::Right);
        }

        // Options card.
        p.rrect(10.0, 362.0, W - 20.0, 146.0, 12.0, CARD, 255);
        p.text(24.0, 380.0, 10.0, DIM, 255, "OPTIONS", Align::Left);
        for (tog, x, cy, small, label) in toggles(Tab::Timer) {
            let (on, enabled) = match tog {
                Tog::SoundOnFocus => (s.sound_on_focus, true),
                Tog::SoundOffAtEnd => (s.sound_off_at_end, true),
                Tog::Chime => (s.chime, true),
                Tog::Overlay => (s.overlay, true),
                _ => (s.overlay_on_top, s.overlay),
            };
            self.toggle(p, tog, x, cy, on, if enabled { TIMER_COLOR } else { DIM }, small);
            let a = if enabled { 255 } else { 120 };
            p.text(x + 32.0, cy + 4.0, 11.0, TEXT, a, label, Align::Left);
        }
        let ov = s.overlay;
        let a = if ov { 255 } else { 120 };
        p.text(24.0, OPT_ROWS[3] + 4.0, 11.0, TEXT, a, "Opacity", Align::Left);
        self.slider(p, SliderId::Opacity, s, TIMER_COLOR, ov);
        p.text(RIGHT, OPT_ROWS[3] + 4.0, 11.0, DIM, a, &format!("{:.0}%", s.overlay_opacity * 100.0), Align::Right);
        p.text(24.0, OPT_ROWS[4] + 4.0, 11.0, TEXT, a, "Size", Align::Left);
        self.slider(p, SliderId::Size, s, TIMER_COLOR, ov);
        p.text(RIGHT, OPT_ROWS[4] + 4.0, 11.0, DIM, a, &format!("{:.1}×", s.overlay_size), Align::Right);
    }

    /// Small coloured bars in the header showing which layers are active.
    fn level_meter(&self, p: &mut Painter, s: &Settings, playing: bool) {
        let layers = [
            (s.noise_on[0], s.noise_vol[0], NOISE_COLORS[0]),
            (s.noise_on[1], s.noise_vol[1], NOISE_COLORS[1]),
            (s.noise_on[2], s.noise_vol[2], NOISE_COLORS[2]),
            (s.bin_on, s.bin_vol, BIN_COLOR),
        ];
        for (i, (on, v, c)) in layers.into_iter().enumerate() {
            let h = 6.0 + 28.0 * if on { v * s.master } else { 0.0 };
            let x = RIGHT - 36.0 + i as f32 * 10.0;
            let a = if playing && on { 255 } else if on { 120 } else { 60 };
            p.rrect(x, 52.0 - h, 6.0, h, 3.0, c, a);
        }
    }

    fn chip(&self, p: &mut Painter, rc: R, sel: bool, enabled: bool, me: Ctrl, c: Rgb) {
        let hov = self.hover == Some(me);
        let (fill, a) = if sel {
            (c, if enabled { 255 } else { 120 })
        } else if hov {
            (HOVER, 255)
        } else {
            (TRACK, 255)
        };
        p.rrect(rc.x, rc.y, rc.w, rc.h, 9.0, fill, a);
    }

    fn toggle(&self, p: &mut Painter, me: Tog, x: f32, cy: f32, on: bool, c: Rgb, small: bool) {
        let hov = self.hover == Some(Ctrl::Toggle(me));
        let (w, h) = if small { (26.0, 14.0) } else { (34.0, 20.0) };
        let fill = if on { c } else if hov { HOVER } else { TRACK };
        p.rrect(x, cy - h / 2.0, w, h, h / 2.0, fill, 255);
        let kr = h / 2.0 - 3.0;
        let kx = if on { x + w - h / 2.0 } else { x + h / 2.0 };
        p.circle(kx, cy, kr, if on { BG } else { DIM }, 255);
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

/// Right-pointing triangle with its left edge at `x`.
fn tri(p: &mut Painter, x: f32, cy: f32, h: f32, c: Rgb) {
    let mut pb = PathBuilder::new();
    pb.move_to(x, cy - h);
    pb.line_to(x + h * 1.2, cy);
    pb.line_to(x, cy + h);
    pb.close();
    p.fill(pb.finish(), c, 255);
}

fn play_pause_icon(p: &mut Painter, cx: f32, cy: f32, k: f32, pause: bool, c: Rgb) {
    if pause {
        p.rrect(cx - 8.0 * k, cy - 9.0 * k, 6.0 * k, 18.0 * k, 1.5, c, 255);
        p.rrect(cx + 2.0 * k, cy - 9.0 * k, 6.0 * k, 18.0 * k, 1.5, c, 255);
    } else {
        tri(p, cx - 6.0 * k, cy, 11.0 * k, c);
    }
}

/// The floating on-screen timer, in logical units of OVERLAY_W × OVERLAY_H.
pub fn draw_overlay(p: &mut Painter, t: &TimerView) {
    p.clear(BG);
    let pc = phase_color(t.phase);
    p.circle(16.0, 17.0, 4.0, pc, 255);
    let mut label = format!("{}  {}/{}", phase_name(t.phase).to_uppercase(), t.round, t.rounds);
    if !t.running {
        label = format!("PAUSED  ·  {label}");
    }
    p.text(26.0, 21.0, 10.0, DIM, 255, &label, Align::Left);
    let a = if t.running { 255 } else { 150 };
    p.text(13.0, 48.0, 27.0, TEXT, a, &clock(t.remaining), Align::Left);
    p.rrect(0.0, OVERLAY_H - 4.0, OVERLAY_W, 4.0, 0.0, TRACK, 255);
    p.rrect(0.0, OVERLAY_H - 4.0, OVERLAY_W * t.progress, 4.0, 0.0, pc, 255);
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
        Self::with_font(epaint_default_fonts::UBUNTU_LIGHT, scale)
    }

    fn with_font(bytes: &[u8], scale: f32) -> Self {
        let font = fontdue::Font::from_bytes(bytes, Default::default()).expect("embedded font");
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

    fn clear(&mut self, c: Rgb) {
        self.pix.fill(tiny_skia::Color::from_rgba8(c.0, c.1, c.2, 255));
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
        if w <= 0.0 || h <= 0.0 {
            return;
        }
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

    fn px(&self, size: f32) -> u16 {
        (size * self.scale).round().max(1.0) as u16
    }

    /// Text width in logical px.
    fn measure(&self, size: f32, s: &str) -> f32 {
        let px = self.px(size) as f32;
        s.chars().map(|ch| self.font.metrics(ch, px).advance_width).sum::<f32>() / self.scale
    }

    /// Draws text with its baseline at `y` (logical px).
    fn text(&mut self, x: f32, y: f32, size: f32, c: Rgb, a: u8, s: &str, align: Align) {
        let px = self.px(size);
        let width = self.measure(size, s) * self.scale;
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

    /// Straight-alpha RGBA, as tray icons expect.
    fn into_rgba(self) -> Vec<u8> {
        self.pix
            .pixels()
            .iter()
            .flat_map(|p| {
                let c = p.demultiply();
                [c.red(), c.green(), c.blue(), c.alpha()]
            })
            .collect()
    }
}

/// Renders the tray icon (three wave bars) as RGBA.
pub fn tray_icon_rgba(size: u32, playing: bool, template: bool) -> Vec<u8> {
    let mut p = Painter::with_font(epaint_default_fonts::HACK_REGULAR, 1.0);
    p.pix = Pixmap::new(size, size).unwrap();
    p.scale = size as f32 / 32.0;
    let c = if template { (0, 0, 0) } else { (255, 255, 255) };
    let a = if playing { 255 } else { 150 };
    for (i, h) in [14.0f32, 24.0, 18.0].into_iter().enumerate() {
        p.rrect(5.0 + i as f32 * 8.5, 16.0 - h / 2.0, 5.0, h, 1.0, c, a);
    }
    p.into_rgba()
}

/// Tray icon showing the minutes left (Windows has no text next to tray icons).
pub fn tray_timer_rgba(size: u32, text: &str, phase: Phase, running: bool) -> Vec<u8> {
    let mut p = Painter::with_font(epaint_default_fonts::HACK_REGULAR, 1.0);
    p.pix = Pixmap::new(size, size).unwrap();
    p.scale = size as f32 / 32.0;
    let bg = if running { phase_color(phase) } else { (110, 116, 130) };
    p.rrect(0.0, 0.0, 32.0, 32.0, 7.0, bg, 255);
    // Biggest size that fits; Hack digits are 0.6 em wide and ~0.73 em tall.
    let fs = (30.0 / (0.6 * text.chars().count() as f32)).min(26.0);
    p.text(16.0, 16.0 + fs * 0.36, fs, (16, 16, 20), 255, text, Align::Center);
    p.into_rgba()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timer_view() -> TimerView {
        TimerView {
            active: true,
            running: true,
            phase: Phase::Focus,
            round: 2,
            rounds: 4,
            remaining: Duration::from_secs(1234),
            progress: 0.4,
        }
    }

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
    fn timer_tab_controls() {
        let mut s = Settings::default();
        let mut ui = Ui::new(Tab::Timer);
        let rc = pomo_preset_rect(1);
        assert_eq!(ui.press(&mut s, rc.x + 5.0, rc.y + 5.0), Response::Changed);
        assert_eq!((s.focus_min, s.break_min), (50.0, 10.0));
        let (x, y, _) = T_MAIN;
        assert_eq!(ui.press(&mut s, x, y), Response::Timer(TimerCmd::StartPause));
        let before = s.overlay;
        let (_, tx, cy, _, _) = toggles(Tab::Timer)[3];
        ui.press(&mut s, tx + 4.0, cy);
        assert_ne!(s.overlay, before);
        let tab = tab_rect(Tab::Sound);
        assert_eq!(ui.press(&mut s, tab.x + 4.0, tab.y + 4.0), Response::Redraw);
        assert_eq!(ui.tab, Tab::Sound);
    }

    #[test]
    fn controls_are_reachable() {
        // Every control must be hit at its own position.
        for tab in [Tab::Sound, Tab::Timer] {
            for &id in sliders(tab) {
                let t = slider_track(id);
                assert_eq!(hit(tab, t.x + t.w / 2.0, t.y + 3.0), Some(Ctrl::Slider(id)), "{id:?}");
            }
            for (t, x, cy, _, _) in toggles(tab) {
                assert_eq!(hit(tab, x + 6.0, cy), Some(Ctrl::Toggle(t)), "{t:?}");
            }
        }
    }

    #[test]
    fn draws_without_panic() {
        for tab in [Tab::Sound, Tab::Timer] {
            let mut p = Painter::new(1.5);
            p.resize((W * 1.5) as u32, (H * 1.5) as u32, 1.5);
            Ui::new(tab).draw(&mut p, &Settings::default(), true, &timer_view(), "hint");
            let mut out = vec![0u32; (p.pix.width() * p.pix.height()) as usize];
            p.blit(&mut out);
            assert!(out.iter().any(|&c| c != out[0]));
        }
        assert_eq!(tray_timer_rgba(32, "25", Phase::Focus, true).len(), 32 * 32 * 4);
    }

    /// `LULL_PREVIEW=dir cargo test preview -- --ignored` renders PNGs.
    #[test]
    #[ignore]
    fn preview() {
        let dir = std::path::PathBuf::from(std::env::var("LULL_PREVIEW").unwrap_or(".".into()));
        let mut s = Settings::default();
        s.bin_on = true;
        s.noise_on[2] = true;
        s.overlay = true;
        let hint = "Ctrl+Alt+N  sound      Ctrl+Alt+P  timer      Ctrl+Alt+L  panel";
        for (tab, name) in [(Tab::Sound, "sound.png"), (Tab::Timer, "timer.png")] {
            let mut p = Painter::new(2.0);
            p.resize((W * 2.0) as u32, (H * 2.0) as u32, 2.0);
            Ui::new(tab).draw(&mut p, &s, true, &timer_view(), hint);
            p.pix.save_png(dir.join(name)).unwrap();
        }
        let mut p = Painter::new(2.0);
        p.resize((OVERLAY_W * 2.0) as u32, (OVERLAY_H * 2.0) as u32, 2.0);
        draw_overlay(&mut p, &timer_view());
        p.pix.save_png(dir.join("overlay.png")).unwrap();
        let rgba = tray_timer_rgba(32, "21", Phase::Focus, true);
        let icon = Pixmap::from_vec(rgba, tiny_skia::IntSize::from_wh(32, 32).unwrap()).unwrap();
        icon.save_png(dir.join("tray.png")).unwrap();
    }
}
