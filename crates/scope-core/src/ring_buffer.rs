/// Fixed-capacity circular buffer for oscilloscope sample storage.
/// One buffer per channel. No heap allocation on the hot path after creation.
pub struct RingBuffer {
    data: Vec<f32>,
    capacity: usize,
    write_pos: usize,
    count: usize,
}

impl RingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            data: vec![0.0; capacity],
            capacity,
            write_pos: 0,
            count: 0,
        }
    }

    pub fn push(&mut self, sample: f32) {
        self.data[self.write_pos] = sample;
        self.write_pos = (self.write_pos + 1) % self.capacity;
        if self.count < self.capacity {
            self.count += 1;
        }
    }

    pub fn push_slice(&mut self, samples: &[f32]) {
        for &s in samples {
            self.push(s);
        }
    }

    /// Get sample at logical index 0 = oldest, count-1 = newest.
    pub fn get(&self, logical_idx: usize) -> Option<f32> {
        if logical_idx >= self.count {
            return None;
        }
        let start = if self.count < self.capacity {
            0
        } else {
            self.write_pos
        };
        let physical = (start + logical_idx) % self.capacity;
        Some(self.data[physical])
    }

    /// Read a contiguous window of samples into the output slice.
    /// Returns the number of samples actually written.
    pub fn read_window(&self, start: usize, out: &mut [f32]) -> usize {
        let mut written = 0;
        for (i, slot) in out.iter_mut().enumerate() {
            if let Some(v) = self.get(start + i) {
                *slot = v;
                written += 1;
            } else {
                break;
            }
        }
        written
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn clear(&mut self) {
        self.write_pos = 0;
        self.count = 0;
    }

    /// Iterate over all samples from oldest to newest.
    pub fn iter(&self) -> RingBufferIter<'_> {
        RingBufferIter {
            buf: self,
            pos: 0,
        }
    }
}

pub struct RingBufferIter<'a> {
    buf: &'a RingBuffer,
    pos: usize,
}

impl Iterator for RingBufferIter<'_> {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let val = self.buf.get(self.pos)?;
        self.pos += 1;
        Some(val)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.buf.count.saturating_sub(self.pos);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for RingBufferIter<'_> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_and_get() {
        let mut rb = RingBuffer::new(4);
        rb.push(1.0);
        rb.push(2.0);
        rb.push(3.0);
        assert_eq!(rb.len(), 3);
        assert_eq!(rb.get(0), Some(1.0));
        assert_eq!(rb.get(2), Some(3.0));
        assert_eq!(rb.get(3), None);
    }

    #[test]
    fn wraparound() {
        let mut rb = RingBuffer::new(3);
        rb.push(1.0);
        rb.push(2.0);
        rb.push(3.0);
        rb.push(4.0); // overwrites 1.0
        assert_eq!(rb.len(), 3);
        assert_eq!(rb.get(0), Some(2.0));
        assert_eq!(rb.get(1), Some(3.0));
        assert_eq!(rb.get(2), Some(4.0));
    }

    #[test]
    fn read_window() {
        let mut rb = RingBuffer::new(8);
        for i in 0..8 {
            rb.push(i as f32);
        }
        let mut buf = [0.0f32; 4];
        let n = rb.read_window(2, &mut buf);
        assert_eq!(n, 4);
        assert_eq!(buf, [2.0, 3.0, 4.0, 5.0]);
    }

    #[test]
    fn iter_order() {
        let mut rb = RingBuffer::new(4);
        for i in 0..6 {
            rb.push(i as f32);
        }
        let vals: Vec<f32> = rb.iter().collect();
        assert_eq!(vals, vec![2.0, 3.0, 4.0, 5.0]);
    }
}
