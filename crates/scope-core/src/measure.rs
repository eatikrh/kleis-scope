/// Auto-measurements computed over a visible window of samples.
#[derive(Debug, Clone, Default)]
pub struct Measurements {
    pub min: f32,
    pub max: f32,
    pub mean: f32,
    pub vpp: f32,
    pub vrms: f32,
    pub frequency: Option<f64>,
    pub period: Option<f64>,
    pub rise_time: Option<f64>,
    pub fall_time: Option<f64>,
    pub sample_count: usize,
}

impl Measurements {
    /// Compute all auto-measurements over a sample window.
    /// `sample_rate` is in Hz.
    pub fn compute(samples: &[f32], sample_rate: f64) -> Self {
        if samples.is_empty() {
            return Self::default();
        }

        let mut min = f32::MAX;
        let mut max = f32::MIN;
        let mut sum = 0.0f64;
        let mut sum_sq = 0.0f64;

        for &s in samples {
            if s < min { min = s; }
            if s > max { max = s; }
            sum += s as f64;
            sum_sq += (s as f64) * (s as f64);
        }

        let n = samples.len() as f64;
        let mean = (sum / n) as f32;
        let vpp = max - min;
        let vrms = ((sum_sq / n) as f32).sqrt();

        let (frequency, period) = compute_frequency(samples, sample_rate, mean);
        let (rise_time, fall_time) = compute_rise_fall(samples, sample_rate, min, max);

        Self {
            min,
            max,
            mean,
            vpp,
            vrms,
            frequency,
            period,
            rise_time,
            fall_time,
            sample_count: samples.len(),
        }
    }
}

/// Zero-crossing frequency estimation.
fn compute_frequency(samples: &[f32], sample_rate: f64, mean: f32) -> (Option<f64>, Option<f64>) {
    if samples.len() < 4 {
        return (None, None);
    }

    let mut crossings = Vec::new();
    for i in 1..samples.len() {
        let prev = samples[i - 1] - mean;
        let curr = samples[i] - mean;
        if prev < 0.0 && curr >= 0.0 {
            let frac = (-prev as f64) / ((curr - prev) as f64);
            crossings.push((i - 1) as f64 + frac);
        }
    }

    if crossings.len() < 2 {
        return (None, None);
    }

    let mut total_period = 0.0;
    let cycles = crossings.len() - 1;
    for i in 1..crossings.len() {
        total_period += crossings[i] - crossings[i - 1];
    }
    let avg_period_samples = total_period / cycles as f64;
    let period = avg_period_samples / sample_rate;
    let frequency = 1.0 / period;

    (Some(frequency), Some(period))
}

/// Estimate 10%-90% rise time and 90%-10% fall time.
fn compute_rise_fall(
    samples: &[f32],
    sample_rate: f64,
    min: f32,
    max: f32,
) -> (Option<f64>, Option<f64>) {
    let range = max - min;
    if range < 1e-12 || samples.len() < 4 {
        return (None, None);
    }

    let low = min + 0.1 * range;
    let high = min + 0.9 * range;

    let mut rise_time = None;
    let mut fall_time = None;

    // Find first rising edge crossing low→high
    let mut i = 0;
    while i < samples.len() - 1 {
        if samples[i] <= low && samples[i + 1] > low {
            let start_idx = i as f64;
            let mut j = i + 1;
            while j < samples.len() {
                if samples[j] >= high {
                    let end_idx = j as f64;
                    rise_time = Some((end_idx - start_idx) / sample_rate);
                    break;
                }
                if samples[j] < low { break; } // aborted
                j += 1;
            }
            if rise_time.is_some() { break; }
        }
        i += 1;
    }

    // Find first falling edge crossing high→low
    i = 0;
    while i < samples.len() - 1 {
        if samples[i] >= high && samples[i + 1] < high {
            let start_idx = i as f64;
            let mut j = i + 1;
            while j < samples.len() {
                if samples[j] <= low {
                    let end_idx = j as f64;
                    fall_time = Some((end_idx - start_idx) / sample_rate);
                    break;
                }
                if samples[j] > high { break; }
                j += 1;
            }
            if fall_time.is_some() { break; }
        }
        i += 1;
    }

    (rise_time, fall_time)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn sine_wave(n: usize, freq: f64, sample_rate: f64) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * PI as f64 * freq * i as f64 / sample_rate).sin() as f32)
            .collect()
    }

    #[test]
    fn basic_measurements() {
        let samples = sine_wave(10000, 100.0, 10000.0);
        let m = Measurements::compute(&samples, 10000.0);
        assert!((m.vpp - 2.0).abs() < 0.01);
        assert!((m.mean).abs() < 0.01);
        assert!((m.vrms - (1.0f32 / 2.0f32.sqrt())).abs() < 0.02);
    }

    #[test]
    fn frequency_measurement() {
        let samples = sine_wave(10000, 440.0, 44100.0);
        let m = Measurements::compute(&samples, 44100.0);
        let freq = m.frequency.unwrap();
        assert!((freq - 440.0).abs() < 2.0, "measured freq={freq}");
    }
}
