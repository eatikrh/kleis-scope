use crate::ring_buffer::RingBuffer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerMode {
    Auto,
    Normal,
    Single,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerEdge {
    Rising,
    Falling,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerState {
    Running,
    Waiting,
    Stopped,
}

pub struct TriggerEngine {
    pub mode: TriggerMode,
    pub edge: TriggerEdge,
    pub level: f32,
    pub source_channel: usize,
    pub holdoff_samples: usize,
    pub state: TriggerState,

    prev_sample: Option<f32>,
    samples_since_trigger: usize,
    triggered_at: Option<usize>,
    auto_timeout_samples: usize,
}

impl TriggerEngine {
    pub fn new() -> Self {
        Self {
            mode: TriggerMode::Auto,
            edge: TriggerEdge::Rising,
            level: 0.0,
            source_channel: 0,
            holdoff_samples: 0,
            state: TriggerState::Running,
            prev_sample: None,
            samples_since_trigger: 0,
            triggered_at: None,
            auto_timeout_samples: 5000,
        }
    }

    /// Check a new sample against the trigger condition.
    /// Returns the buffer index where the trigger fired, if it did.
    pub fn check(&mut self, sample: f32, buffer_len: usize) -> Option<usize> {
        if self.state == TriggerState::Stopped {
            return None;
        }

        self.samples_since_trigger += 1;
        let result = if self.samples_since_trigger > self.holdoff_samples {
            if let Some(prev) = self.prev_sample {
                let crossed = match self.edge {
                    TriggerEdge::Rising => prev < self.level && sample >= self.level,
                    TriggerEdge::Falling => prev > self.level && sample <= self.level,
                };
                if crossed {
                    self.samples_since_trigger = 0;
                    let idx = buffer_len.saturating_sub(1);
                    self.triggered_at = Some(idx);
                    if self.mode == TriggerMode::Single {
                        self.state = TriggerState::Stopped;
                    }
                    Some(idx)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        self.prev_sample = Some(sample);

        if result.is_none() && self.mode == TriggerMode::Auto {
            if self.samples_since_trigger >= self.auto_timeout_samples {
                self.samples_since_trigger = 0;
                let idx = buffer_len.saturating_sub(1);
                self.triggered_at = Some(idx);
                return Some(idx);
            }
        }

        result
    }

    /// Find the trigger point by scanning the buffer from the end backwards.
    /// Used when we need to re-locate trigger position after scrolling.
    pub fn find_trigger_in_buffer(&self, buf: &RingBuffer) -> Option<usize> {
        if buf.len() < 2 {
            return None;
        }
        let start = buf.len().saturating_sub(self.auto_timeout_samples);
        for i in (start + 1..buf.len()).rev() {
            let curr = buf.get(i)?;
            let prev = buf.get(i - 1)?;
            let crossed = match self.edge {
                TriggerEdge::Rising => prev < self.level && curr >= self.level,
                TriggerEdge::Falling => prev > self.level && curr <= self.level,
            };
            if crossed {
                return Some(i);
            }
        }
        None
    }

    pub fn last_trigger_index(&self) -> Option<usize> {
        self.triggered_at
    }

    pub fn arm(&mut self) {
        self.state = TriggerState::Waiting;
        self.triggered_at = None;
        self.samples_since_trigger = 0;
    }

    pub fn force_trigger(&mut self, buffer_len: usize) {
        self.triggered_at = Some(buffer_len.saturating_sub(1));
        self.samples_since_trigger = 0;
        if self.mode == TriggerMode::Single {
            self.state = TriggerState::Stopped;
        }
    }

    pub fn set_auto_timeout(&mut self, samples: usize) {
        self.auto_timeout_samples = samples;
    }
}

impl Default for TriggerEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rising_edge_trigger() {
        let mut trig = TriggerEngine::new();
        trig.level = 0.5;
        trig.edge = TriggerEdge::Rising;

        assert!(trig.check(-1.0, 1).is_none());
        assert!(trig.check(0.0, 2).is_none());
        assert!(trig.check(0.3, 3).is_none());
        let result = trig.check(0.6, 4);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), 3);
    }

    #[test]
    fn falling_edge_trigger() {
        let mut trig = TriggerEngine::new();
        trig.level = 0.5;
        trig.edge = TriggerEdge::Falling;

        assert!(trig.check(1.0, 1).is_none());
        assert!(trig.check(0.8, 2).is_none());
        let result = trig.check(0.3, 3);
        assert!(result.is_some());
    }

    #[test]
    fn single_mode_stops() {
        let mut trig = TriggerEngine::new();
        trig.mode = TriggerMode::Single;
        trig.level = 0.5;
        trig.edge = TriggerEdge::Rising;

        trig.check(0.0, 1);
        trig.check(1.0, 2); // triggers
        assert_eq!(trig.state, TriggerState::Stopped);
        assert!(trig.check(0.0, 3).is_none());
        assert!(trig.check(1.0, 4).is_none()); // stays stopped
    }

    #[test]
    fn auto_timeout_fires() {
        let mut trig = TriggerEngine::new();
        trig.level = 100.0; // will never cross
        trig.set_auto_timeout(5);

        for i in 0..4 {
            assert!(trig.check(0.0, i + 1).is_none());
        }
        let result = trig.check(0.0, 5);
        assert!(result.is_some());
    }

    #[test]
    fn holdoff_suppresses() {
        let mut trig = TriggerEngine::new();
        trig.level = 0.5;
        trig.holdoff_samples = 10;
        trig.set_auto_timeout(100000);

        trig.check(0.0, 1);
        trig.check(1.0, 2); // would trigger but holdoff hasn't expired
        // holdoff starts at 0, so the first trigger at sample 2 fires because
        // samples_since_trigger starts > holdoff (both 0)
        // Let's verify holdoff after a trigger:
        trig.samples_since_trigger = 0; // simulate post-trigger
        trig.prev_sample = Some(0.0);
        for i in 0..9 {
            assert!(trig.check(if i % 2 == 0 { 1.0 } else { 0.0 }, 10 + i).is_none());
        }
    }
}
