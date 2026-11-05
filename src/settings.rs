//! User settings, persisted as a tiny `key=value` text file.

use std::path::PathBuf;
use std::time::Duration;

use crate::pomodoro::Plan;

pub const NOISE_NAMES: [&str; 3] = ["White", "Pink", "Brown"];

/// Binaural presets: (name, symbol, beat Hz).
pub const PRESETS: [(&str, &str, f32); 4] = [
    ("Delta", "δ", 2.0),
    ("Theta", "θ", 6.0),
    ("Alpha", "α", 10.0),
    ("Beta", "β", 20.0),
];

/// Pomodoro presets: (focus, break, long break, rounds), minutes.
pub const POMO_PRESETS: [(f32, f32, f32, f32); 4] = [
    (25.0, 5.0, 15.0, 4.0),
    (50.0, 10.0, 30.0, 3.0),
    (52.0, 17.0, 30.0, 3.0),
    (90.0, 20.0, 30.0, 2.0),
];

pub const BEAT_RANGE: (f32, f32) = (0.5, 40.0);
pub const CARRIER_RANGE: (f32, f32) = (60.0, 600.0);
pub const FOCUS_RANGE: (f32, f32) = (1.0, 120.0);
pub const BREAK_RANGE: (f32, f32) = (1.0, 60.0);
pub const ROUNDS_RANGE: (f32, f32) = (1.0, 8.0);
pub const OPACITY_RANGE: (f32, f32) = (0.2, 1.0);
pub const OVERLAY_SIZE_RANGE: (f32, f32) = (0.6, 3.0);

#[derive(Clone, PartialEq, Debug)]
pub struct Settings {
    pub master: f32,
    pub noise_on: [bool; 3],
    pub noise_vol: [f32; 3],
    pub bin_on: bool,
    pub bin_vol: f32,
    pub beat: f32,
    pub carrier: f32,
    /// Global shortcuts, e.g. `ctrl+alt+KeyN` (see the README).
    pub hotkey_play: String,
    pub hotkey_panel: String,
    pub hotkey_timer: String,

    // Pomodoro, in minutes.
    pub focus_min: f32,
    pub break_min: f32,
    pub long_min: f32,
    pub rounds: f32,
    /// Start the sound when a focus session starts (and pause it with the timer).
    pub sound_on_focus: bool,
    /// Stop the sound when a focus session (or the whole timer) ends.
    pub sound_off_at_end: bool,
    pub chime: bool,

    // On-screen timer.
    pub overlay: bool,
    pub overlay_on_top: bool,
    pub overlay_opacity: f32,
    pub overlay_size: f32,
    /// Physical screen position; `None` until the user moves it.
    pub overlay_pos: Option<(i32, i32)>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            master: 0.7,
            noise_on: [false, true, false],
            noise_vol: [0.5, 0.6, 0.6],
            bin_on: false,
            bin_vol: 0.5,
            beat: 10.0,
            carrier: 200.0,
            hotkey_play: "ctrl+alt+KeyN".into(),
            hotkey_panel: "ctrl+alt+KeyL".into(),
            hotkey_timer: "ctrl+alt+KeyP".into(),
            focus_min: 25.0,
            break_min: 5.0,
            long_min: 15.0,
            rounds: 4.0,
            sound_on_focus: true,
            sound_off_at_end: true,
            chime: true,
            overlay: false,
            overlay_on_top: true,
            overlay_opacity: 0.85,
            overlay_size: 1.0,
            overlay_pos: None,
        }
    }
}

fn path() -> Option<PathBuf> {
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"));
    #[cfg(not(any(windows, target_os = "macos")))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    base.map(|b| b.join("lull").join("settings.txt"))
}

impl Settings {
    pub fn load() -> Self {
        let text = path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        Self::parse(&text)
    }

    fn parse(text: &str) -> Self {
        let mut s = Settings::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let v = v.trim();
            let f = v.parse::<f32>().ok().filter(|f| f.is_finite());
            let b = v == "1";
            match (k.trim(), f) {
                ("master", Some(f)) => s.master = f,
                ("white", Some(f)) => s.noise_vol[0] = f,
                ("pink", Some(f)) => s.noise_vol[1] = f,
                ("brown", Some(f)) => s.noise_vol[2] = f,
                ("binaural", Some(f)) => s.bin_vol = f,
                ("beat", Some(f)) => s.beat = f,
                ("carrier", Some(f)) => s.carrier = f,
                ("white_on", _) => s.noise_on[0] = b,
                ("pink_on", _) => s.noise_on[1] = b,
                ("brown_on", _) => s.noise_on[2] = b,
                ("binaural_on", _) => s.bin_on = b,
                ("hotkey_play", _) => s.hotkey_play = v.into(),
                ("hotkey_panel", _) => s.hotkey_panel = v.into(),
                ("hotkey_timer", _) => s.hotkey_timer = v.into(),
                ("focus_min", Some(f)) => s.focus_min = f,
                ("break_min", Some(f)) => s.break_min = f,
                ("long_min", Some(f)) => s.long_min = f,
                ("rounds", Some(f)) => s.rounds = f,
                ("sound_on_focus", _) => s.sound_on_focus = b,
                ("sound_off_at_end", _) => s.sound_off_at_end = b,
                ("chime", _) => s.chime = b,
                ("overlay", _) => s.overlay = b,
                ("overlay_on_top", _) => s.overlay_on_top = b,
                ("overlay_opacity", Some(f)) => s.overlay_opacity = f,
                ("overlay_size", Some(f)) => s.overlay_size = f,
                ("overlay_pos", _) => {
                    s.overlay_pos = v
                        .split_once(',')
                        .and_then(|(x, y)| Some((x.trim().parse().ok()?, y.trim().parse().ok()?)))
                }
                _ => {}
            }
        }
        s.clamp()
    }

    fn clamp(mut self) -> Self {
        let c = |v: f32, (lo, hi): (f32, f32)| v.clamp(lo, hi);
        self.master = c(self.master, (0.0, 1.0));
        self.bin_vol = c(self.bin_vol, (0.0, 1.0));
        self.noise_vol = self.noise_vol.map(|v| c(v, (0.0, 1.0)));
        self.beat = c(self.beat, BEAT_RANGE);
        self.carrier = c(self.carrier, CARRIER_RANGE);
        self.focus_min = c(self.focus_min, FOCUS_RANGE).round();
        self.break_min = c(self.break_min, BREAK_RANGE).round();
        self.long_min = c(self.long_min, BREAK_RANGE).round();
        self.rounds = c(self.rounds, ROUNDS_RANGE).round();
        self.overlay_opacity = c(self.overlay_opacity, OPACITY_RANGE);
        self.overlay_size = c(self.overlay_size, OVERLAY_SIZE_RANGE);
        self
    }

    pub fn plan(&self) -> Plan {
        let m = |x: f32| Duration::from_secs((x * 60.0) as u64);
        Plan {
            focus: m(self.focus_min),
            short: m(self.break_min),
            long: m(self.long_min),
            rounds: self.rounds as u32,
        }
    }

    fn serialize(&self) -> String {
        let b = |x: bool| x as u8;
        let pos = self.overlay_pos.map(|(x, y)| format!("{x},{y}")).unwrap_or_default();
        let lines = [
            format!("master={}", self.master),
            format!("white={}", self.noise_vol[0]),
            format!("pink={}", self.noise_vol[1]),
            format!("brown={}", self.noise_vol[2]),
            format!("binaural={}", self.bin_vol),
            format!("beat={}", self.beat),
            format!("carrier={}", self.carrier),
            format!("white_on={}", b(self.noise_on[0])),
            format!("pink_on={}", b(self.noise_on[1])),
            format!("brown_on={}", b(self.noise_on[2])),
            format!("binaural_on={}", b(self.bin_on)),
            format!("hotkey_play={}", self.hotkey_play),
            format!("hotkey_panel={}", self.hotkey_panel),
            format!("hotkey_timer={}", self.hotkey_timer),
            format!("focus_min={}", self.focus_min),
            format!("break_min={}", self.break_min),
            format!("long_min={}", self.long_min),
            format!("rounds={}", self.rounds),
            format!("sound_on_focus={}", b(self.sound_on_focus)),
            format!("sound_off_at_end={}", b(self.sound_off_at_end)),
            format!("chime={}", b(self.chime)),
            format!("overlay={}", b(self.overlay)),
            format!("overlay_on_top={}", b(self.overlay_on_top)),
            format!("overlay_opacity={}", self.overlay_opacity),
            format!("overlay_size={}", self.overlay_size),
            format!("overlay_pos={pos}"),
        ];
        lines.join("\n") + "\n"
    }

    pub fn save(&self) {
        let Some(p) = path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, self.serialize());
    }
}

/// Turns `ctrl+alt+KeyN` into `Ctrl+Alt+N` (`Ctrl+Opt+N` on macOS).
pub fn pretty_hotkey(spec: &str) -> String {
    let mac = cfg!(target_os = "macos");
    spec.split('+')
        .map(|t| match t.trim().to_ascii_lowercase().as_str() {
            "ctrl" | "control" => "Ctrl".to_string(),
            "alt" | "option" => (if mac { "Opt" } else { "Alt" }).to_string(),
            "shift" => "Shift".to_string(),
            "super" | "cmd" | "command" | "meta" => (if mac { "Cmd" } else { "Win" }).to_string(),
            "cmdorctrl" | "commandorcontrol" => (if mac { "Cmd" } else { "Ctrl" }).to_string(),
            _ => {
                let t = t.trim();
                t.strip_prefix("Key").or_else(|| t.strip_prefix("Digit")).unwrap_or(t).to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let mut s = Settings::default();
        s.overlay_pos = Some((-120, 40));
        s.focus_min = 50.0;
        s.overlay = true;
        s.noise_on = [true, false, true];
        assert_eq!(Settings::parse(&s.serialize()), s);
    }

    #[test]
    fn bad_values_are_clamped() {
        let s = Settings::parse("focus_min=999\nrounds=0\noverlay_opacity=nan\noverlay_pos=abc");
        assert_eq!(s.focus_min, 120.0);
        assert_eq!(s.rounds, 1.0);
        assert_eq!(s.overlay_opacity, Settings::default().overlay_opacity);
        assert_eq!(s.overlay_pos, None);
    }
}
