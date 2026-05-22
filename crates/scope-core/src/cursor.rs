/// Paired cursors for time and voltage measurement on the oscilloscope display.
#[derive(Debug, Clone)]
pub struct CursorPair {
    pub enabled: bool,
    pub a: f64,
    pub b: f64,
}

impl CursorPair {
    pub fn new() -> Self {
        Self {
            enabled: false,
            a: 0.0,
            b: 0.0,
        }
    }

    pub fn delta(&self) -> f64 {
        (self.b - self.a).abs()
    }

    pub fn mid(&self) -> f64 {
        (self.a + self.b) / 2.0
    }
}

impl Default for CursorPair {
    fn default() -> Self {
        Self::new()
    }
}

/// Full cursor state: two time cursors and two voltage cursors.
pub struct Cursors {
    pub time: CursorPair,
    pub voltage: CursorPair,
}

impl Cursors {
    pub fn new() -> Self {
        Self {
            time: CursorPair::new(),
            voltage: CursorPair::new(),
        }
    }

    /// Frequency implied by time cursor delta (1/dt).
    pub fn time_delta_frequency(&self) -> Option<f64> {
        let dt = self.time.delta();
        if dt > 1e-15 { Some(1.0 / dt) } else { None }
    }
}

impl Default for Cursors {
    fn default() -> Self {
        Self::new()
    }
}
