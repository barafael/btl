use std::f32::consts::TAU;
use std::io::Write;
use std::path::Path;

// --- LCG RNG ---

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 32) as i32 as f32 / 2147483648.0
    }
}

// --- DSP helpers ---

fn lowpass(signal: &[f32], cutoff_hz: f32, sample_rate: f32) -> Vec<f32> {
    let a = 1.0 / (1.0 + sample_rate / (TAU * cutoff_hz));
    let mut out = vec![0.0f32; signal.len()];
    let mut y = 0.0f32;
    for (i, &x) in signal.iter().enumerate() {
        y = y + a * (x - y);
        out[i] = y;
    }
    out
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

// --- WAV writer ---

fn write_wav(path: &Path, samples: &[f32]) {
    const SAMPLE_RATE: u32 = 44100;
    const NUM_CHANNELS: u16 = 1;
    const BITS_PER_SAMPLE: u16 = 16;
    let byte_rate = SAMPLE_RATE * u32::from(NUM_CHANNELS) * u32::from(BITS_PER_SAMPLE) / 8;
    let block_align = NUM_CHANNELS * BITS_PER_SAMPLE / 8;
    let data_size = (samples.len() * 2) as u32;
    let chunk_size = 36 + data_size;

    let mut file = std::fs::File::create(path).expect("create wav");

    // RIFF header
    file.write_all(b"RIFF").unwrap();
    file.write_all(&chunk_size.to_le_bytes()).unwrap();
    file.write_all(b"WAVE").unwrap();

    // fmt chunk
    file.write_all(b"fmt ").unwrap();
    file.write_all(&16u32.to_le_bytes()).unwrap(); // chunk size
    file.write_all(&1u16.to_le_bytes()).unwrap(); // PCM
    file.write_all(&NUM_CHANNELS.to_le_bytes()).unwrap();
    file.write_all(&SAMPLE_RATE.to_le_bytes()).unwrap();
    file.write_all(&byte_rate.to_le_bytes()).unwrap();
    file.write_all(&block_align.to_le_bytes()).unwrap();
    file.write_all(&BITS_PER_SAMPLE.to_le_bytes()).unwrap();

    // data chunk
    file.write_all(b"data").unwrap();
    file.write_all(&data_size.to_le_bytes()).unwrap();
    for &s in samples {
        let clamped = s.clamp(-1.0, 1.0);
        let pcm = (clamped * 32767.0) as i16;
        file.write_all(&pcm.to_le_bytes()).unwrap();
    }
}

fn normalize(samples: &mut Vec<f32>) {
    let peak = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    if peak > 0.98 {
        let scale = 0.98 / peak;
        for s in samples.iter_mut() {
            *s *= scale;
        }
    }
}

// --- Sound generators ---

fn gen_engine_hum(sr: f32) -> Vec<f32> {
    let duration = 2.0_f32;
    let n = (sr * duration) as usize;
    let mut samples = vec![0.0f32; n];
    for i in 0..n {
        let t = i as f32 / sr;
        // Core harmonics
        let core = (TAU * 80.0 * t).sin()
            + 0.5 * (TAU * 160.0 * t).sin()
            + 0.25 * (TAU * 240.0 * t).sin()
            + 0.12 * (TAU * 320.0 * t).sin()
            + 0.06 * (TAU * 480.0 * t).sin();
        // Detuned oscillator for chorus/beating
        let detune = 0.3 * (TAU * 80.3 * t).sin();
        // Tremolo
        let tremolo = 1.0 + 0.05 * (TAU * 6.5 * t).sin();
        samples[i] = (core + detune) * tremolo * 0.45;
    }
    normalize(&mut samples);
    samples
}

fn gen_ambient_drone(sr: f32) -> Vec<f32> {
    let duration = 8.0_f32;
    let n = (sr * duration) as usize;
    let mut rng = Rng(0xdeadbeef);
    let noise: Vec<f32> = (0..n).map(|_| rng.next()).collect();
    let filtered_noise = lowpass(&noise, 100.0, sr);
    let mut samples = vec![0.0f32; n];
    for i in 0..n {
        let t = i as f32 / sr;
        let sub = 0.35 * (TAU * 25.0 * t).sin();
        let mid = 0.18 * (TAU * 37.0 * t).sin();
        let noise_comp = filtered_noise[i] * 0.12;
        samples[i] = (sub + mid + noise_comp) * 0.5;
    }
    normalize(&mut samples);
    samples
}

fn gen_autocannon(sr: f32) -> Vec<f32> {
    let duration = 0.07_f32;
    let n = (sr * duration) as usize;
    let mut rng = Rng(0xaabbccdd);
    let mut samples = vec![0.0f32; n];
    for i in 0..n {
        let t = i as f32 / sr;
        let noise = rng.next();
        let burst = noise * (-t * 120.0).exp() * 0.7;
        let ring = 0.35 * (TAU * 920.0 * t).sin() * (-t * 90.0).exp();
        samples[i] = burst + ring;
    }
    normalize(&mut samples);
    samples
}

fn gen_heavy_cannon(sr: f32) -> Vec<f32> {
    let duration = 0.35_f32;
    let n = (sr * duration) as usize;
    let mut rng = Rng(0x11223344);
    let mut samples = vec![0.0f32; n];
    for i in 0..n {
        let t = i as f32 / sr;
        let attack = 1.0 - (-t * 300.0).exp();
        let body = 0.55 * (TAU * 55.0 * t).sin() * (-t * 9.0).exp();
        let harmonic = 0.3 * (TAU * 88.0 * t).sin() * (-t * 14.0).exp();
        let transient = rng.next() * (-t * 80.0).exp() * 0.35;
        samples[i] = (body + harmonic + transient) * attack;
    }
    normalize(&mut samples);
    samples
}

fn gen_laser_loop(sr: f32) -> Vec<f32> {
    let duration = 1.0_f32;
    let n = (sr * duration) as usize;
    let mut rng = Rng(0x55667788);
    let noise: Vec<f32> = (0..n).map(|_| rng.next()).collect();
    let filtered_noise = lowpass(&noise, 2000.0, sr);
    let mut samples = vec![0.0f32; n];
    for i in 0..n {
        let t = i as f32 / sr;
        let saw1 = (t * 380.0).fract() * 2.0 - 1.0;
        let saw2 = (t * 382.0).fract() * 2.0 - 1.0;
        let am = 0.5 + 0.5 * (TAU * 50.0 * t).sin();
        let noise_comp = filtered_noise[i] * 0.08;
        samples[i] = (saw1 * 0.4 + saw2 * 0.35 + noise_comp) * am * 0.6;
    }
    normalize(&mut samples);
    samples
}

fn gen_torpedo_launch(sr: f32) -> Vec<f32> {
    let duration = 0.55_f32;
    let n = (sr * duration) as usize;
    let mut rng = Rng(0x99aabbcc);
    let noise: Vec<f32> = (0..n).map(|_| rng.next()).collect();
    // bandpass-ish: highpass to cut sub-bass for torpedo body sound
    let hp_noise: Vec<f32> = {
        let lp_low = lowpass(&noise, 150.0, sr);
        let lp_high = lowpass(&noise, 1800.0, sr);
        lp_high.iter().zip(lp_low.iter()).map(|(h, l)| h - l).collect()
    };
    let mut samples = vec![0.0f32; n];
    let mut phase = 0.0_f32;
    let freq_start = 200.0_f32;
    let freq_end = 520.0_f32;
    let sweep_end = 0.45_f32;
    for i in 0..n {
        let t = i as f32 / sr;
        let freq = if t < sweep_end {
            freq_start + (freq_end - freq_start) * (t / sweep_end)
        } else {
            freq_end
        };
        phase += TAU * freq / sr;
        let env = (1.0 - (-t * 40.0).exp()) * (-(((t - 0.5) / 0.1).max(0.0)) * 15.0).exp();
        let sine_comp = phase.sin() * 0.45;
        let noise_comp = hp_noise[i] * 0.4;
        samples[i] = (sine_comp + noise_comp) * env;
    }
    normalize(&mut samples);
    samples
}

fn gen_railgun_charge(sr: f32) -> Vec<f32> {
    let duration = 2.5_f32;
    let n = (sr * duration) as usize;
    let mut samples = vec![0.0f32; n];
    let mut phase = 0.0_f32;
    for i in 0..n {
        let t = i as f32 / sr;
        let frac = t / 2.5;
        let freq = 100.0 * (1600.0_f32 / 100.0).powf(frac);
        phase += TAU * freq / sr;
        let h2 = 0.4 * frac;
        let h3 = 0.2 * frac;
        let env = 0.1 + 0.8 * frac.powi(2);
        samples[i] = (phase.sin() + h2 * (phase * 2.0).sin() + h3 * (phase * 3.0).sin()) * env;
    }
    normalize(&mut samples);
    samples
}

fn gen_railgun_fire(sr: f32) -> Vec<f32> {
    let duration = 0.28_f32;
    let n = (sr * duration) as usize;
    let mut rng = Rng(0xffeeddcc);
    let mut samples = vec![0.0f32; n];
    let mut sweep_phase = 0.0_f32;
    for i in 0..n {
        let t = i as f32 / sr;
        let noise = rng.next();
        let crack = noise * (-t * 500.0).exp() * 0.8;
        // exponential descending sweep 2000 -> 150 Hz
        let sweep_freq = 2000.0 * (150.0_f32 / 2000.0).powf(t / 0.28);
        sweep_phase += TAU * sweep_freq / sr;
        let sweep = sweep_phase.sin() * (-t * 18.0).exp() * 0.6;
        let rumble = 0.3 * (TAU * 55.0 * t).sin() * (-t * 12.0).exp();
        samples[i] = (crack + sweep + rumble) * 0.85;
    }
    normalize(&mut samples);
    samples
}

fn gen_explosion_large(sr: f32) -> Vec<f32> {
    let duration = 1.0_f32;
    let n = (sr * duration) as usize;
    let mut rng = Rng(0xcafe1234);
    let noise_raw: Vec<f32> = (0..n).map(|_| rng.next()).collect();
    let lp_noise = lowpass(&noise_raw, 300.0, sr);
    let mut samples = vec![0.0f32; n];
    for i in 0..n {
        let t = i as f32 / sr;
        let full = noise_raw[i] * (-t * 5.5).exp() * 0.5;
        let lp = lp_noise[i] * (-t * 4.0).exp() * 0.55;
        let sub = 0.6 * (TAU * 42.0 * t).sin() * (-t * 3.5).exp();
        let mid = 0.4 * (TAU * 75.0 * t).sin() * (-t * 5.0).exp();
        samples[i] = (full + lp + sub + mid).clamp(-1.0, 1.0);
    }
    normalize(&mut samples);
    samples
}

fn gen_explosion_medium(sr: f32) -> Vec<f32> {
    let duration = 0.6_f32;
    let n = (sr * duration) as usize;
    let mut rng = Rng(0xbeef5678);
    let noise_raw: Vec<f32> = (0..n).map(|_| rng.next()).collect();
    let lp_noise = lowpass(&noise_raw, 300.0, sr);
    let mut samples = vec![0.0f32; n];
    for i in 0..n {
        let t = i as f32 / sr;
        let full = noise_raw[i] * (-t * 8.25).exp() * 0.5;
        let lp = lp_noise[i] * (-t * 6.0).exp() * 0.55;
        let sub = 0.35 * (TAU * 42.0 * t).sin() * (-t * 5.25).exp();
        let mid = 0.4 * (TAU * 75.0 * t).sin() * (-t * 7.5).exp();
        samples[i] = (full + lp + sub + mid).clamp(-1.0, 1.0);
    }
    normalize(&mut samples);
    samples
}

fn gen_zone_capture(sr: f32) -> Vec<f32> {
    let duration = 0.38_f32;
    let n = (sr * duration) as usize;
    let mut samples = vec![0.0f32; n];
    for i in 0..n {
        let t = i as f32 / sr;
        // Note 1: 440 Hz, starts at 0.0
        let t1 = t;
        let env1 = (-t1 * 6.0).exp() * smoothstep(0.0, 0.01, t1);
        let note1 = ((TAU * 440.0 * t1).sin() + 0.2 * (TAU * 880.0 * t1).sin()) * env1 * 0.45;

        // Note 2: 659 Hz, starts at 0.18
        let note2 = if t >= 0.18 {
            let t2 = t - 0.18;
            let env2 = (-t2 * 6.0).exp() * smoothstep(0.0, 0.01, t2);
            ((TAU * 659.0 * t2).sin() + 0.2 * (TAU * 1318.0 * t2).sin()) * env2 * 0.45
        } else {
            0.0
        };
        samples[i] = note1 + note2;
    }
    normalize(&mut samples);
    samples
}

fn gen_zone_flip(sr: f32) -> Vec<f32> {
    let duration = 0.65_f32;
    let n = (sr * duration) as usize;
    let notes = [(0.0_f32, 440.0_f32), (0.18, 554.0), (0.36, 659.0)];
    let mut samples = vec![0.0f32; n];
    for i in 0..n {
        let t = i as f32 / sr;
        let mut s = 0.0_f32;
        for &(start, freq) in &notes {
            if t >= start {
                let tl = t - start;
                let env = (-tl * 5.0).exp();
                s += ((TAU * freq * tl).sin()
                    + 0.15 * (TAU * freq * 2.0 * tl).sin()
                    + 0.07 * (TAU * freq * 3.0 * tl).sin())
                    * env
                    * 0.4;
            }
        }
        samples[i] = s;
    }
    normalize(&mut samples);
    samples
}

fn gen_respawn(sr: f32) -> Vec<f32> {
    let duration = 0.48_f32;
    let n = (sr * duration) as usize;
    let notes = [(0.0_f32, 523.0_f32), (0.14, 659.0), (0.28, 784.0)];
    let mut samples = vec![0.0f32; n];
    for i in 0..n {
        let t = i as f32 / sr;
        let mut s = 0.0_f32;
        for &(start, freq) in &notes {
            if t >= start {
                let tl = t - start;
                let env = (-tl * 7.0).exp() * smoothstep(0.0, 0.005, tl);
                s += ((TAU * freq * tl).sin() + 0.3 * (TAU * freq * 2.76 * tl).sin())
                    * env
                    * 0.38;
            }
        }
        samples[i] = s;
    }
    normalize(&mut samples);
    samples
}

// --- Main ---

fn main() {
    let out_dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "crates/btl-client/assets/audio".to_string());
    let out_path = Path::new(&out_dir);
    std::fs::create_dir_all(out_path).expect("create output directory");

    const SR: f32 = 44100.0;

    let sounds: &[(&str, Vec<f32>)] = &[
        ("engine_hum.wav", gen_engine_hum(SR)),
        ("ambient_drone.wav", gen_ambient_drone(SR)),
        ("autocannon.wav", gen_autocannon(SR)),
        ("heavy_cannon.wav", gen_heavy_cannon(SR)),
        ("laser_loop.wav", gen_laser_loop(SR)),
        ("torpedo_launch.wav", gen_torpedo_launch(SR)),
        ("railgun_charge.wav", gen_railgun_charge(SR)),
        ("railgun_fire.wav", gen_railgun_fire(SR)),
        ("explosion_large.wav", gen_explosion_large(SR)),
        ("explosion_medium.wav", gen_explosion_medium(SR)),
        ("zone_capture.wav", gen_zone_capture(SR)),
        ("zone_flip.wav", gen_zone_flip(SR)),
        ("respawn.wav", gen_respawn(SR)),
    ];

    for (filename, samples) in sounds {
        let file_path = out_path.join(filename);
        write_wav(&file_path, samples);
        println!("Generated: {filename}");
    }
}
