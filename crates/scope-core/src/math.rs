use rustfft::{FftPlanner, num_complex::Complex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathOp {
    Add,
    Subtract,
    Multiply,
    Fft,
}

/// Compute a math channel from two source channels.
pub fn compute_math(a: &[f32], b: &[f32], op: MathOp) -> Vec<f32> {
    let len = a.len().min(b.len());
    match op {
        MathOp::Add => (0..len).map(|i| a[i] + b[i]).collect(),
        MathOp::Subtract => (0..len).map(|i| a[i] - b[i]).collect(),
        MathOp::Multiply => (0..len).map(|i| a[i] * b[i]).collect(),
        MathOp::Fft => compute_fft_magnitude(a),
    }
}

/// Compute FFT magnitude spectrum (in dB) from a real-valued signal.
/// Returns N/2+1 bins covering 0 to Nyquist.
pub fn compute_fft_magnitude(signal: &[f32]) -> Vec<f32> {
    let n = signal.len();
    if n == 0 {
        return Vec::new();
    }

    // Round up to next power of two for efficiency
    let fft_size = n.next_power_of_two();

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(fft_size);

    let mut buffer: Vec<Complex<f32>> = signal
        .iter()
        .map(|&s| Complex::new(s, 0.0))
        .collect();
    // Zero-pad
    buffer.resize(fft_size, Complex::new(0.0, 0.0));

    // Apply Hann window to reduce spectral leakage
    for (i, sample) in buffer.iter_mut().enumerate().take(n) {
        let w = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (n - 1) as f32).cos());
        sample.re *= w;
    }

    fft.process(&mut buffer);

    let n_bins = fft_size / 2 + 1;
    let norm = 1.0 / fft_size as f32;

    buffer[..n_bins]
        .iter()
        .map(|c| {
            let mag = (c.re * c.re + c.im * c.im).sqrt() * norm;
            20.0 * (mag.max(1e-12)).log10() // dB
        })
        .collect()
}

/// Frequency of each FFT bin given sample_rate and fft_size.
pub fn fft_bin_frequency(bin: usize, fft_size: usize, sample_rate: f64) -> f64 {
    bin as f64 * sample_rate / fft_size as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    #[test]
    fn fft_detects_sine() {
        let sample_rate = 1000.0;
        let freq = 100.0;
        let n: usize = 1024;
        let signal: Vec<f32> = (0..n)
            .map(|i| (2.0 * PI * freq as f32 * i as f32 / sample_rate as f32).sin())
            .collect();

        let spectrum = compute_fft_magnitude(&signal);
        assert!(!spectrum.is_empty());

        // Find peak bin
        let peak_bin = spectrum
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap()
            .0;

        let fft_size = n.next_power_of_two();
        let peak_freq = fft_bin_frequency(peak_bin, fft_size, sample_rate);
        assert!(
            (peak_freq - freq as f64).abs() < 5.0,
            "peak at {peak_freq} Hz, expected {freq} Hz"
        );
    }

    #[test]
    fn math_ops() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![0.5, 1.0, 1.5];
        assert_eq!(compute_math(&a, &b, MathOp::Add), vec![1.5, 3.0, 4.5]);
        assert_eq!(compute_math(&a, &b, MathOp::Subtract), vec![0.5, 1.0, 1.5]);
    }
}
