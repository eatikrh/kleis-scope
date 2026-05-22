use crate::ring_buffer::RingBuffer;

const DEFAULT_BUFFER_CAPACITY: usize = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coupling {
    DC,
    AC,
    GND,
}

#[derive(Debug, Clone, Copy)]
pub struct ChannelColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl ChannelColor {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub fn to_rgba(&self) -> [u8; 4] {
        [self.r, self.g, self.b, 255]
    }
}

pub const COLORS: [ChannelColor; 4] = [
    ChannelColor::new(64, 255, 64),   // phosphor green
    ChannelColor::new(255, 255, 64),  // yellow
    ChannelColor::new(64, 200, 255),  // cyan
    ChannelColor::new(255, 64, 255),  // magenta
];

/// Per-channel state mirroring a real oscilloscope channel knob panel.
pub struct Channel {
    pub label: String,
    pub enabled: bool,
    pub coupling: Coupling,
    pub volts_per_div: f32,
    pub offset: f32,
    pub color: ChannelColor,
    pub buffer: RingBuffer,
}

impl Channel {
    pub fn new(index: usize) -> Self {
        Self {
            label: format!("CH{}", index + 1),
            enabled: true,
            coupling: Coupling::DC,
            volts_per_div: 1.0,
            offset: 0.0,
            color: COLORS[index % COLORS.len()],
            buffer: RingBuffer::new(DEFAULT_BUFFER_CAPACITY),
        }
    }

    pub fn push_sample(&mut self, raw: f32) {
        let value = match self.coupling {
            Coupling::DC => raw,
            Coupling::AC => raw, // AC coupling needs a high-pass filter state; deferred
            Coupling::GND => 0.0,
        };
        self.buffer.push(value);
    }

    /// Convert a raw sample value to screen-space divisions (centered at 0).
    pub fn value_to_divs(&self, value: f32) -> f32 {
        (value - self.offset) / self.volts_per_div
    }
}

/// Standard 1-2-5 sequence for V/div and time/div knobs.
pub const SCALE_1_2_5: [f32; 12] = [
    0.001, 0.002, 0.005,
    0.01, 0.02, 0.05,
    0.1, 0.2, 0.5,
    1.0, 2.0, 5.0,
];

pub fn next_scale_up(current: f32) -> f32 {
    for &s in &SCALE_1_2_5 {
        if s > current * 1.01 {
            return s;
        }
    }
    current * 2.0
}

pub fn next_scale_down(current: f32) -> f32 {
    for &s in SCALE_1_2_5.iter().rev() {
        if s < current * 0.99 {
            return s;
        }
    }
    current * 0.5
}
