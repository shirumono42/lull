//! Audio engine: white / pink / brown noise, binaural beats and a timer chime.
//!
//! The output stream only exists while sound is needed. Stopping fades out and
//! then drops the stream, so an idle app holds no audio device and uses no CPU.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};

use crate::settings::Settings;

const FADE_OUT: Duration = Duration::from_millis(350);
const CHIME_LEN: f32 = 2.2;

#[derive(Default)]
struct AtomicF32(AtomicU32);

impl AtomicF32 {
    fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Relaxed))
    }
    fn set(&self, v: f32) {
        self.0.store(v.to_bits(), Relaxed)
    }
}

/// Parameters shared lock-free between the UI thread and the audio callback.
/// Gains are stored already mapped (on/off and curve applied).
#[derive(Default)]
struct Shared {
    running: AtomicBool,
    master: AtomicF32,
    noise: [AtomicF32; 3],
    binaural: AtomicF32,
    beat: AtomicF32,
    carrier: AtomicF32,
    /// Bumped to ring the chime once.
    chime: AtomicU32,
}

pub struct Engine {
    shared: Arc<Shared>,
    stream: Option<cpal::Stream>,
    stop_at: Option<Instant>,
}

impl Engine {
    pub fn new(s: &Settings) -> Self {
        let e = Engine { shared: Arc::default(), stream: None, stop_at: None };
        e.apply(s);
        e
    }

    /// Push UI settings to the audio thread.
    pub fn apply(&self, s: &Settings) {
        let sh = &self.shared;
        let curve = |on: bool, v: f32| if on { v * v } else { 0.0 };
        sh.master.set(curve(true, s.master));
        for i in 0..3 {
            sh.noise[i].set(curve(s.noise_on[i], s.noise_vol[i]));
        }
        sh.binaural.set(curve(s.bin_on, s.bin_vol));
        sh.beat.set(s.beat);
        sh.carrier.set(s.carrier);
    }

    pub fn is_playing(&self) -> bool {
        self.stream.is_some() && self.shared.running.load(Relaxed)
    }

    pub fn toggle(&mut self) {
        if self.is_playing() { self.stop() } else { self.start() }
    }

    pub fn start(&mut self) {
        self.stop_at = None;
        self.shared.running.store(true, Relaxed);
        if !self.ensure_stream() {
            self.shared.running.store(false, Relaxed);
        }
    }

    pub fn stop(&mut self) {
        if self.is_playing() {
            self.shared.running.store(false, Relaxed);
            self.stop_at = Some(Instant::now() + FADE_OUT);
        }
    }

    /// Rings a short bell, opening the stream just for it if needed.
    pub fn chime(&mut self) {
        // Open the stream first: a new stream only rings chimes requested
        // after it was built.
        if !self.is_playing() && self.ensure_stream() {
            let end = Instant::now() + Duration::from_secs_f32(CHIME_LEN);
            self.stop_at = Some(self.stop_at.map_or(end, |t| t.max(end)));
        }
        self.shared.chime.fetch_add(1, Relaxed);
    }

    fn ensure_stream(&mut self) -> bool {
        if self.stream.is_none() {
            match build_stream(self.shared.clone()) {
                Ok(s) => {
                    self.stream = Some(s);
                    #[cfg(windows)]
                    opt_out_of_ducking();
                }
                Err(e) => eprintln!("lull: audio error: {e}"),
            }
        }
        self.stream.is_some()
    }

    /// Drops the stream once the fade-out is over. Returns the next deadline.
    pub fn poll(&mut self) -> Option<Instant> {
        match self.stop_at {
            Some(t) if Instant::now() >= t => {
                self.stream = None;
                self.stop_at = None;
                None
            }
            other => other,
        }
    }
}

/// Windows lowers "other sounds" whenever a voice-chat app is active. That
/// silently changes Lull's level by several dB, so opt this session out.
#[cfg(windows)]
fn opt_out_of_ducking() {
    use windows::Win32::Media::Audio::{
        IAudioSessionControl2, IAudioSessionManager, IMMDeviceEnumerator, MMDeviceEnumerator,
        eConsole, eRender,
    };
    use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx};
    use windows::core::Interface;
    unsafe {
        // The UI thread is already in an STA; this is a no-op then.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let result = (|| -> windows::core::Result<()> {
            let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let dev = en.GetDefaultAudioEndpoint(eRender, eConsole)?;
            let mgr: IAudioSessionManager = dev.Activate(CLSCTX_ALL, None)?;
            let ctl = mgr.GetAudioSessionControl(None, 0)?;
            ctl.cast::<IAudioSessionControl2>()?.SetDuckingPreference(true)
        })();
        if let Err(e) = result {
            eprintln!("lull: could not opt out of ducking: {e}");
        }
    }
}

fn build_stream(shared: Arc<Shared>) -> Result<cpal::Stream, String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("no output device")?;
    let config = device.default_output_config().map_err(|e| e.to_string())?;
    let format = config.sample_format();
    let config: cpal::StreamConfig = config.into();
    let stream = match format {
        SampleFormat::F32 => run::<f32>(&device, config, shared),
        SampleFormat::I16 => run::<i16>(&device, config, shared),
        SampleFormat::U16 => run::<u16>(&device, config, shared),
        SampleFormat::I32 => run::<i32>(&device, config, shared),
        SampleFormat::F64 => run::<f64>(&device, config, shared),
        f => return Err(format!("unsupported sample format {f}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

fn run<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    shared: Arc<Shared>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels as usize;
    let mut synth = Synth::new(config.sample_rate as f32);
    // Chimes rung before this stream existed are not replayed.
    synth.chime_seen = shared.chime.load(Relaxed);
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                synth.load(&shared);
                for frame in data.chunks_mut(channels) {
                    let (l, r) = synth.next();
                    match frame {
                        [mono] => *mono = T::from_sample((l + r) * 0.5),
                        [a, b, rest @ ..] => {
                            *a = T::from_sample(l);
                            *b = T::from_sample(r);
                            rest.iter_mut().for_each(|s| *s = T::from_sample(0.0));
                        }
                        [] => {}
                    }
                }
            },
            |e| eprintln!("lull: stream error: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
}

/// Per-channel noise generator state.
#[derive(Default, Clone, Copy)]
struct NoiseState {
    pink: [f32; 7],
    brown: f32,
    // DC blocker state (input, output).
    hp: (f32, f32),
}

impl NoiseState {
    /// Returns (white, pink, brown) at roughly comparable loudness.
    #[inline]
    fn next(&mut self, w: f32) -> [f32; 3] {
        // Paul Kellet's refined pink noise filter.
        let b = &mut self.pink;
        b[0] = 0.99886 * b[0] + w * 0.0555179;
        b[1] = 0.99332 * b[1] + w * 0.0750759;
        b[2] = 0.96900 * b[2] + w * 0.1538520;
        b[3] = 0.86650 * b[3] + w * 0.3104856;
        b[4] = 0.55000 * b[4] + w * 0.5329522;
        b[5] = -0.7616 * b[5] - w * 0.0168980;
        let pink = (b[0] + b[1] + b[2] + b[3] + b[4] + b[5] + b[6] + w * 0.5362) * 0.11;
        b[6] = w * 0.115926;
        // Leaky integrator for brown (red) noise.
        self.brown = (self.brown + 0.02 * w) / 1.02;
        [w * 0.35, pink, self.brown * 3.5]
    }

    /// One-pole high-pass around 20 Hz. Pink and brown noise carry a lot of
    /// inaudible sub-20 Hz energy that makes the signal level wander by ~2 dB;
    /// OS loudness equalisers and headphone limiters react to that wander and
    /// audibly pump the volume up and down.
    #[inline]
    fn high_pass(&mut self, x: f32, r: f32) -> f32 {
        let y = x - self.hp.0 + r * self.hp.1;
        self.hp = (x, y);
        y
    }
}

/// Transparent below the knee, smoothly saturating above it.
#[inline]
fn soft_limit(x: f32) -> f32 {
    const KNEE: f32 = 0.8;
    let a = x.abs();
    if a <= KNEE {
        x
    } else {
        let over = (a - KNEE) / (1.0 - KNEE);
        (KNEE + (1.0 - KNEE) * over / (1.0 + over)).copysign(x)
    }
}

struct Synth {
    sr: f32,
    rng: u64,
    ch: [NoiseState; 2],
    phase: [f32; 2],
    // Targets read once per buffer, smoothed per sample to avoid clicks.
    target: [f32; 6],
    gain: [f32; 6],
    beat: f32,
    carrier: f32,
    smooth: f32,
    hp_r: f32,
    chime_seen: u32,
    chime_t: u32,
}

impl Synth {
    fn new(sr: f32) -> Self {
        Synth {
            sr,
            rng: 0x9E37_79B9_7F4A_7C15,
            ch: [NoiseState::default(); 2],
            phase: [0.0; 2],
            target: [0.0; 6],
            gain: [0.0; 6],
            beat: 10.0,
            carrier: 200.0,
            smooth: 1.0 - (-1.0 / (0.04 * sr)).exp(),
            hp_r: 1.0 - std::f32::consts::TAU * 20.0 / sr,
            chime_seen: 0,
            chime_t: u32::MAX,
        }
    }

    fn load(&mut self, s: &Shared) {
        let fade = if s.running.load(Relaxed) { 1.0 } else { 0.0 };
        self.target = [
            s.noise[0].get(),
            s.noise[1].get(),
            s.noise[2].get(),
            s.binaural.get(),
            s.master.get(),
            fade,
        ];
        self.beat = s.beat.get();
        self.carrier = s.carrier.get();
        let chime = s.chime.load(Relaxed);
        if chime != self.chime_seen {
            self.chime_seen = chime;
            self.chime_t = 0;
        }
    }

    /// Uniform in [-1, 1) from xorshift64*.
    #[inline]
    fn rand(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        let bits = (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as u32; // 24 bits
        bits as f32 * (2.0 / (1u32 << 24) as f32) - 1.0
    }

    /// Soft two-partial bell, independent of the noise fade.
    #[inline]
    fn chime(&mut self) -> f32 {
        let len = (CHIME_LEN * self.sr) as u32;
        if self.chime_t >= len {
            return 0.0;
        }
        let t = self.chime_t as f32 / self.sr;
        self.chime_t += 1;
        let attack = (t / 0.005).min(1.0);
        let env = attack * (-t / 0.45).exp();
        let w = std::f32::consts::TAU * t;
        env * 0.22 * ((w * 784.0).sin() + 0.5 * (w * 1176.0).sin())
    }

    #[inline]
    fn next(&mut self) -> (f32, f32) {
        for (g, t) in self.gain.iter_mut().zip(self.target) {
            *g += (t - *g) * self.smooth;
        }
        let g = self.gain;
        let bell = self.chime() * g[4].max(0.15);
        let mut out = [0.0f32; 2];
        let freqs = [self.carrier - self.beat * 0.5, self.carrier + self.beat * 0.5];
        for c in 0..2 {
            let w = self.rand();
            let [wh, pk, br] = self.ch[c].next(w);
            let noise = self.ch[c].high_pass(wh * g[0] + pk * g[1] + br * g[2], self.hp_r);
            let p = &mut self.phase[c];
            *p = (*p + freqs[c] / self.sr).fract();
            let tone = (*p * std::f32::consts::TAU).sin() * 0.3;
            let mix = (noise + tone * g[3]) * g[4] * g[5] + bell;
            out[c] = soft_limit(mix);
        }
        if !(out[0].is_finite() && out[1].is_finite()) {
            // Never let a bad value latch into the filters and play on.
            self.ch = [NoiseState::default(); 2];
            self.phase = [0.0; 2];
            self.chime_t = u32::MAX;
            return (0.0, 0.0);
        }
        (out[0], out[1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(idx: usize) -> f32 {
        let mut s = Synth::new(48_000.0);
        let mut st = NoiseState::default();
        let n = 480_000;
        let sum: f32 = (0..n).map(|_| { let w = s.rand(); st.next(w)[idx].powi(2) }).sum();
        (sum / n as f32).sqrt()
    }

    #[test]
    fn noise_levels_are_balanced_and_bounded() {
        for i in 0..3 {
            let r = rms(i);
            assert!((0.08..0.35).contains(&r), "noise {i} rms {r}");
        }
    }

    #[test]
    fn rand_is_centred() {
        let mut s = Synth::new(48_000.0);
        let mean: f32 = (0..100_000).map(|_| s.rand()).sum::<f32>() / 100_000.0;
        assert!(mean.abs() < 0.01);
    }

    /// Regression test for the "volume drifts by a few dB" bug: the short-term
    /// level of the output must stay steady at fixed settings.
    #[test]
    fn short_term_level_is_steady() {
        for gains in [[0.0, 0.36, 0.0], [0.0, 0.0, 0.36], [0.25, 0.36, 0.36]] {
            let sr = 48_000.0;
            let mut s = Synth::new(sr);
            s.target = [gains[0], gains[1], gains[2], 0.0, 0.49, 1.0];
            s.gain = s.target;
            let win = (0.4 * sr) as usize;
            for _ in 0..win {
                s.next(); // let the filters settle
            }
            let db: Vec<f64> = (0..150)
                .map(|_| {
                    let acc: f64 = (0..win).map(|_| (s.next().0 as f64).powi(2)).sum();
                    10.0 * (acc / win as f64).log10()
                })
                .collect();
            let mean = db.iter().sum::<f64>() / db.len() as f64;
            let worst = db.iter().map(|d| (d - mean).abs()).fold(0.0, f64::max);
            assert!(worst < 1.5, "{gains:?}: level wanders by {worst:.2} dB");
        }
    }

    #[test]
    fn soft_limit_is_bounded_and_transparent() {
        assert_eq!(soft_limit(0.5), 0.5);
        assert!(soft_limit(5.0) < 1.0 && soft_limit(-5.0) > -1.0);
    }

    /// After the sound is stopped and a chime has rung out, the output must be
    /// true silence: nothing may keep sounding after the timer ends.
    #[test]
    fn silent_after_stop_and_chime() {
        let sr = 48_000.0;
        let mut s = Synth::new(sr);
        let sh = Shared::default();
        sh.running.store(true, Relaxed);
        sh.master.set(0.5);
        for g in &sh.noise {
            g.set(0.4);
        }
        sh.binaural.set(0.4);
        sh.beat.set(6.0);
        sh.carrier.set(200.0);
        for block in 0..(sr as usize * 6 / 512) {
            if block == (sr as usize * 2 / 512) {
                sh.running.store(false, Relaxed); // focus ended
                sh.chime.fetch_add(1, Relaxed); // chime at the same moment
            }
            s.load(&sh);
            for _ in 0..512 {
                let (l, r) = s.next();
                assert!(l.is_finite() && r.is_finite());
                if block > sr as usize * 5 / 512 {
                    assert!(l.abs() < 1e-4 && r.abs() < 1e-4, "still sounding: {l}");
                }
            }
        }
    }

    #[test]
    fn new_stream_does_not_replay_old_chimes() {
        let sh = Shared::default();
        sh.chime.fetch_add(3, Relaxed);
        let mut s = Synth::new(48_000.0);
        s.chime_seen = sh.chime.load(Relaxed);
        s.load(&sh);
        let energy: f32 = (0..4_800).map(|_| s.next().0.abs()).sum();
        assert_eq!(energy, 0.0);
    }

    #[test]
    fn chime_rings_once() {
        let mut s = Synth::new(48_000.0);
        let sh = Shared::default();
        sh.chime.fetch_add(1, Relaxed);
        s.load(&sh);
        let energy: f32 = (0..48_000).map(|_| s.next().0.abs()).sum();
        assert!(energy > 100.0);
        s.load(&sh);
        let tail: f32 = (0..200_000).map(|_| s.next().0.abs()).sum::<f32>();
        assert!(tail < energy);
    }
}
