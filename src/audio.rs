//! Audio engine: white / pink / brown noise and binaural beats.
//!
//! The output stream only exists while playing. Stopping fades out and then
//! drops the stream, so an idle app holds no audio device and uses no CPU.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};

use crate::settings::Settings;

const FADE_OUT: Duration = Duration::from_millis(350);

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
        self.stream.is_some() && self.stop_at.is_none()
    }

    pub fn toggle(&mut self) {
        if self.is_playing() { self.stop() } else { self.start() }
    }

    pub fn start(&mut self) {
        self.stop_at = None;
        self.shared.running.store(true, Relaxed);
        if self.stream.is_none() {
            match build_stream(self.shared.clone()) {
                Ok(s) => self.stream = Some(s),
                Err(e) => {
                    eprintln!("lull: audio error: {e}");
                    self.shared.running.store(false, Relaxed);
                }
            }
        }
    }

    pub fn stop(&mut self) {
        if self.stream.is_some() && self.stop_at.is_none() {
            self.shared.running.store(false, Relaxed);
            self.stop_at = Some(Instant::now() + FADE_OUT);
        }
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
}

struct Synth {
    sr: f32,
    rng: u32,
    ch: [NoiseState; 2],
    phase: [f32; 2],
    // Targets read once per buffer, smoothed per sample to avoid clicks.
    target: [f32; 6],
    gain: [f32; 6],
    beat: f32,
    carrier: f32,
    smooth: f32,
}

impl Synth {
    fn new(sr: f32) -> Self {
        Synth {
            sr,
            rng: 0x9E37_79B9,
            ch: [NoiseState::default(); 2],
            phase: [0.0; 2],
            target: [0.0; 6],
            gain: [0.0; 6],
            beat: 10.0,
            carrier: 200.0,
            smooth: 1.0 - (-1.0 / (0.04 * sr)).exp(),
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
    }

    #[inline]
    fn rand(&mut self) -> f32 {
        // xorshift32
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    #[inline]
    fn next(&mut self) -> (f32, f32) {
        for (g, t) in self.gain.iter_mut().zip(self.target) {
            *g += (t - *g) * self.smooth;
        }
        let g = self.gain;
        let mut out = [0.0f32; 2];
        let freqs = [self.carrier - self.beat * 0.5, self.carrier + self.beat * 0.5];
        for c in 0..2 {
            let w = self.rand();
            let [wh, pk, br] = self.ch[c].next(w);
            let p = &mut self.phase[c];
            *p = (*p + freqs[c] / self.sr).fract();
            let tone = (*p * std::f32::consts::TAU).sin() * 0.3;
            let mix = wh * g[0] + pk * g[1] + br * g[2] + tone * g[3];
            out[c] = (mix * g[4] * g[5]).clamp(-1.0, 1.0);
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
}
