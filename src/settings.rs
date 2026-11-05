//! User settings, persisted as a tiny `key=value` text file.

use std::path::PathBuf;

pub const NOISE_NAMES: [&str; 3] = ["White", "Pink", "Brown"];

/// Binaural presets: (name, symbol, beat Hz).
pub const PRESETS: [(&str, &str, f32); 4] = [
    ("Delta", "δ", 2.0),
    ("Theta", "θ", 6.0),
    ("Alpha", "α", 10.0),
    ("Beta", "β", 20.0),
];

pub const BEAT_RANGE: (f32, f32) = (0.5, 40.0);
pub const CARRIER_RANGE: (f32, f32) = (60.0, 600.0);

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Settings {
    pub master: f32,
    pub noise_on: [bool; 3],
    pub noise_vol: [f32; 3],
    pub bin_on: bool,
    pub bin_vol: f32,
    pub beat: f32,
    pub carrier: f32,
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
        let mut s = Settings::default();
        let Some(text) = path().and_then(|p| std::fs::read_to_string(p).ok()) else {
            return s;
        };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let f = v.trim().parse::<f32>().ok().filter(|f| f.is_finite());
            let b = v.trim() == "1";
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
                _ => {}
            }
        }
        s.clamp()
    }

    fn clamp(mut self) -> Self {
        self.master = self.master.clamp(0.0, 1.0);
        self.bin_vol = self.bin_vol.clamp(0.0, 1.0);
        self.noise_vol = self.noise_vol.map(|v| v.clamp(0.0, 1.0));
        self.beat = self.beat.clamp(BEAT_RANGE.0, BEAT_RANGE.1);
        self.carrier = self.carrier.clamp(CARRIER_RANGE.0, CARRIER_RANGE.1);
        self
    }

    pub fn save(&self) {
        let Some(p) = path() else { return };
        let b = |x: bool| x as u8;
        let text = format!(
            "master={}\nwhite={}\npink={}\nbrown={}\nbinaural={}\nbeat={}\ncarrier={}\n\
             white_on={}\npink_on={}\nbrown_on={}\nbinaural_on={}\n",
            self.master,
            self.noise_vol[0],
            self.noise_vol[1],
            self.noise_vol[2],
            self.bin_vol,
            self.beat,
            self.carrier,
            b(self.noise_on[0]),
            b(self.noise_on[1]),
            b(self.noise_on[2]),
            b(self.bin_on),
        );
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, text);
    }
}
