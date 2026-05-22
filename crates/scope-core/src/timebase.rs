/// Timebase maps ring buffer indices to screen-space horizontal position.
/// Mirrors the horizontal controls on a real oscilloscope.
pub struct Timebase {
    pub sample_rate: f64,
    pub time_per_div: f64,
    pub horizontal_position: f64,
    pub num_horizontal_divs: u32,
}

impl Timebase {
    pub fn new(sample_rate: f64) -> Self {
        let time_per_div = 10.0 / sample_rate; // ~10 samples per division initially
        Self {
            sample_rate,
            time_per_div,
            horizontal_position: 0.0,
            num_horizontal_divs: 10,
        }
    }

    /// Total time span visible on screen.
    pub fn visible_duration(&self) -> f64 {
        self.time_per_div * self.num_horizontal_divs as f64
    }

    /// Number of samples that fit in the visible window.
    pub fn visible_samples(&self) -> usize {
        (self.visible_duration() * self.sample_rate).ceil() as usize
    }

    /// Given a sample count in the buffer, return the logical start index
    /// for the visible window (accounting for horizontal position).
    pub fn window_start(&self, buffer_len: usize) -> usize {
        let vis = self.visible_samples();
        if buffer_len <= vis {
            return 0;
        }
        let end = buffer_len;
        let offset_samples = (self.horizontal_position * self.sample_rate) as isize;
        let start = (end as isize - vis as isize - offset_samples).max(0) as usize;
        start.min(buffer_len.saturating_sub(vis))
    }

    /// Convert a sample index (relative to window start) to a horizontal
    /// division coordinate (0.0 = left edge, num_horizontal_divs = right edge).
    pub fn sample_to_div(&self, sample_in_window: usize) -> f64 {
        let t = sample_in_window as f64 / self.sample_rate;
        t / self.time_per_div
    }

    /// Convert a division coordinate back to a time offset from window start.
    pub fn div_to_time(&self, div: f64) -> f64 {
        div * self.time_per_div
    }

    pub fn zoom_in(&mut self) {
        self.time_per_div *= 0.5;
        self.clamp_time_per_div();
    }

    pub fn zoom_out(&mut self) {
        self.time_per_div *= 2.0;
        self.clamp_time_per_div();
    }

    fn clamp_time_per_div(&mut self) {
        let min_tpd = 1.0 / self.sample_rate; // one sample per div
        let max_tpd = 100.0; // 100 seconds per div
        self.time_per_div = self.time_per_div.clamp(min_tpd, max_tpd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_samples_basic() {
        let tb = Timebase::new(1000.0);
        let vis = tb.visible_samples();
        assert!(vis > 0);
        let expected_duration = tb.time_per_div * 10.0;
        let expected_samples = (expected_duration * 1000.0).ceil() as usize;
        assert_eq!(vis, expected_samples);
    }

    #[test]
    fn zoom_clamps() {
        let mut tb = Timebase::new(1000.0);
        for _ in 0..100 {
            tb.zoom_in();
        }
        assert!(tb.time_per_div >= 1.0 / 1000.0);
    }
}
