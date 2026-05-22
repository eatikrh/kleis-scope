use std::sync::mpsc;
use std::path::PathBuf;

use clap::Parser;
use eframe::egui;
use scope_core::channel::Channel;
use scope_core::cursor::Cursors;
use scope_core::input::SampleFrame;
use scope_core::measure::Measurements;
use scope_core::timebase::Timebase;
use scope_core::trigger::{TriggerEdge, TriggerEngine, TriggerMode};

#[derive(Parser, Debug)]
#[command(name = "kleis-scope", about = "Standalone oscilloscope")]
struct Cli {
    /// Read CSV samples from stdin
    #[arg(long)]
    stdin: bool,

    /// Connect to a WebSocket URL for streaming data
    #[arg(long)]
    ws: Option<String>,

    /// Connect to a TCP address (host:port) for streaming data
    #[arg(long)]
    tcp: Option<String>,

    /// Tail a file for new samples
    #[arg(long)]
    file: Option<PathBuf>,

    /// Number of channels to expect
    #[arg(long, default_value = "2")]
    channels: usize,

    /// Sample rate in Hz
    #[arg(long, default_value = "10000")]
    rate: f64,

    /// First CSV column is a timestamp
    #[arg(long)]
    has_timestamp: bool,

    /// Start in XY mode (Ch1→X, Ch2→Y)
    #[arg(long)]
    xy: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisplayMode {
    YT,
    XY,
}

/// Real oscilloscope sweep state machine.
/// Armed → trigger fires → Filling (collect post-trigger) → Displaying (frozen) → re-arm.
#[derive(Debug)]
enum SweepState {
    /// Waiting for trigger to fire.
    Armed,
    /// Trigger fired at monotonic sample count `trigger_at`.
    /// Collecting `remaining` more post-trigger samples to fill the screen.
    Filling { trigger_at: u64, remaining: usize },
    /// Display is frozen. `trigger_at` lets us compute the anchor.
    /// `hold_remaining` counts down before re-arming (Auto mode only).
    Displaying { trigger_at: u64, hold_remaining: usize },
}

struct ScopeApp {
    channels: Vec<Channel>,
    timebase: Timebase,
    trigger: TriggerEngine,
    cursors: Cursors,
    measurements: Vec<Measurements>,
    rx: mpsc::Receiver<SampleFrame>,
    show_measurements: bool,
    show_cursors: bool,
    _rt: Option<tokio::runtime::Runtime>,
    /// Monotonically increasing sample counter (never resets).
    total_samples: u64,
    sweep: SweepState,
    /// Fraction of the visible window to show before the trigger point.
    pre_trigger_fraction: f32,
    display_mode: DisplayMode,
    /// How many samples to show as the XY trail.
    xy_trail_length: usize,
}

impl ScopeApp {
    fn new(cli: Cli) -> Self {
        let n_ch = cli.channels.max(1);
        let channels: Vec<Channel> = (0..n_ch).map(Channel::new).collect();
        let timebase = Timebase::new(cli.rate);
        let trigger = TriggerEngine::new();
        let cursors = Cursors::new();
        let measurements = vec![Measurements::default(); n_ch];

        let rt = if cli.ws.is_some() || cli.tcp.is_some() {
            Some(
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("tokio runtime"),
            )
        } else {
            None
        };

        let rx = if cli.stdin {
            scope_core::input::stdin::spawn_stdin_reader(cli.has_timestamp)
        } else if let Some(url) = &cli.ws {
            scope_core::input::websocket::spawn_ws_reader(
                url.clone(),
                cli.has_timestamp,
                rt.as_ref().unwrap(),
            )
        } else if let Some(addr) = &cli.tcp {
            scope_core::input::tcp::spawn_tcp_reader(
                addr.clone(),
                cli.has_timestamp,
                rt.as_ref().unwrap(),
            )
        } else if let Some(path) = &cli.file {
            scope_core::input::file::spawn_file_reader(path.clone(), cli.has_timestamp)
        } else {
            // No input source — create a dummy channel that never sends
            let (_tx, rx) = mpsc::channel();
            rx
        };

        Self {
            channels,
            timebase,
            trigger,
            cursors,
            measurements,
            rx,
            show_measurements: true,
            show_cursors: false,
            _rt: rt,
            total_samples: 0,
            sweep: SweepState::Armed,
            pre_trigger_fraction: 0.2,
            display_mode: if cli.xy { DisplayMode::XY } else { DisplayMode::YT },
            xy_trail_length: 20_000,
        }
    }

    fn ingest_samples(&mut self) {
        let mut count = 0;
        while let Ok(frame) = self.rx.try_recv() {
            for (i, &val) in frame.values.iter().enumerate() {
                if i < self.channels.len() {
                    self.channels[i].push_sample(val);
                }
            }
            self.total_samples += 1;

            let src_ch = self.trigger.source_channel;
            let trigger_val = if src_ch < self.channels.len() {
                frame.values.get(src_ch).copied()
            } else {
                None
            };

            match &mut self.sweep {
                SweepState::Armed => {
                    if let Some(val) = trigger_val {
                        let buf_len = self.channels[src_ch].buffer.len();
                        if self.trigger.check(val, buf_len).is_some() {
                            let vis = self.timebase.visible_samples();
                            let post = vis - (vis as f32 * self.pre_trigger_fraction) as usize;
                            self.sweep = SweepState::Filling {
                                trigger_at: self.total_samples,
                                remaining: post,
                            };
                        }
                    }
                }
                SweepState::Filling { remaining, trigger_at } => {
                    let trigger_at_val = *trigger_at;
                    if *remaining <= 1 {
                        let vis = self.timebase.visible_samples();
                        self.sweep = SweepState::Displaying {
                            trigger_at: trigger_at_val,
                            hold_remaining: vis,
                        };
                    } else {
                        *remaining -= 1;
                    }
                    if let Some(val) = trigger_val {
                        let buf_len = self.channels[src_ch].buffer.len();
                        self.trigger.check(val, buf_len);
                    }
                }
                SweepState::Displaying { hold_remaining, trigger_at } => {
                    let trigger_at_val = *trigger_at;
                    if let Some(val) = trigger_val {
                        let buf_len = self.channels[src_ch].buffer.len();
                        self.trigger.check(val, buf_len);
                    }
                    match self.trigger.mode {
                        TriggerMode::Auto => {
                            if *hold_remaining <= 1 {
                                self.trigger.arm();
                                self.sweep = SweepState::Armed;
                            } else {
                                *hold_remaining -= 1;
                            }
                        }
                        TriggerMode::Normal => {
                            if *hold_remaining <= 1 {
                                self.trigger.arm();
                                self.sweep = SweepState::Armed;
                            } else {
                                *hold_remaining -= 1;
                            }
                        }
                        TriggerMode::Single => {
                            let _ = trigger_at_val;
                            // stay frozen
                        }
                    }
                }
            }

            count += 1;
            if count > 50_000 { break; }
        }
    }

    /// Compute the display window start index using monotonic sample counter.
    /// In Displaying/Filling state, anchor to the trigger point.
    /// In Armed state, show the latest data (free-running).
    fn display_window_start(&self, buffer_len: usize) -> usize {
        let trigger_at = match &self.sweep {
            SweepState::Displaying { trigger_at, .. } => Some(*trigger_at),
            SweepState::Filling { trigger_at, .. } => Some(*trigger_at),
            SweepState::Armed => None,
        };

        if let Some(trig_count) = trigger_at {
            let samples_ago = self.total_samples.saturating_sub(trig_count) as usize;
            if samples_ago >= buffer_len {
                return self.timebase.window_start(buffer_len);
            }
            let trigger_buf_idx = buffer_len - 1 - samples_ago;
            let vis = self.timebase.visible_samples();
            let pre_trigger = (vis as f32 * self.pre_trigger_fraction) as usize;
            let start = trigger_buf_idx.saturating_sub(pre_trigger);
            if start + vis <= buffer_len {
                start
            } else {
                buffer_len.saturating_sub(vis)
            }
        } else {
            self.timebase.window_start(buffer_len)
        }
    }

    fn update_measurements(&mut self) {
        for (i, ch) in self.channels.iter().enumerate() {
            if !ch.enabled || ch.buffer.is_empty() {
                continue;
            }
            let start = self.display_window_start(ch.buffer.len());
            let vis = self.timebase.visible_samples().min(ch.buffer.len().saturating_sub(start));
            let mut buf = vec![0.0f32; vis];
            ch.buffer.read_window(start, &mut buf);
            if i < self.measurements.len() {
                self.measurements[i] = Measurements::compute(&buf, self.timebase.sample_rate);
            }
        }
    }
}

impl eframe::App for ScopeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ingest_samples();
        self.update_measurements();

        ctx.request_repaint();

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("kleis-scope");
                ui.separator();

                let xy_label = if self.display_mode == DisplayMode::XY { "⏹ XY" } else { "▷ XY" };
                if ui.button(xy_label).clicked() {
                    self.display_mode = match self.display_mode {
                        DisplayMode::YT => DisplayMode::XY,
                        DisplayMode::XY => DisplayMode::YT,
                    };
                }
                ui.separator();

                ui.label("Trigger:");
                egui::ComboBox::from_id_salt("trig_mode")
                    .selected_text(match self.trigger.mode {
                        TriggerMode::Auto => "Auto",
                        TriggerMode::Normal => "Normal",
                        TriggerMode::Single => "Single",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.trigger.mode, TriggerMode::Auto, "Auto");
                        ui.selectable_value(&mut self.trigger.mode, TriggerMode::Normal, "Normal");
                        ui.selectable_value(&mut self.trigger.mode, TriggerMode::Single, "Single");
                    });

                egui::ComboBox::from_id_salt("trig_edge")
                    .selected_text(match self.trigger.edge {
                        TriggerEdge::Rising => "Rising",
                        TriggerEdge::Falling => "Falling",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.trigger.edge, TriggerEdge::Rising, "Rising");
                        ui.selectable_value(&mut self.trigger.edge, TriggerEdge::Falling, "Falling");
                    });

                ui.add(egui::DragValue::new(&mut self.trigger.level).speed(0.01).prefix("Level: "));
                ui.separator();

                if ui.button("Force").clicked() {
                    if let Some(ch) = self.channels.get(self.trigger.source_channel) {
                        self.trigger.force_trigger(ch.buffer.len());
                    }
                }
                if ui.button("Arm").clicked() {
                    self.trigger.arm();
                }

                ui.separator();
                ui.checkbox(&mut self.show_measurements, "Meas");
                ui.checkbox(&mut self.show_cursors, "Cursors");
            });
        });

        egui::SidePanel::right("channel_panel").min_width(180.0).show(ctx, |ui| {
            ui.heading("Channels");
            ui.separator();
            for ch in &mut self.channels {
                ui.horizontal(|ui| {
                    let color32 = egui::Color32::from_rgb(ch.color.r, ch.color.g, ch.color.b);
                    ui.colored_label(color32, &ch.label);
                    ui.checkbox(&mut ch.enabled, "");
                });
                ui.horizontal(|ui| {
                    ui.label("V/div:");
                    ui.add(egui::DragValue::new(&mut ch.volts_per_div).speed(0.01).range(0.001..=100.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Offset:");
                    ui.add(egui::DragValue::new(&mut ch.offset).speed(0.01));
                });
                ui.separator();
            }

            if self.display_mode == DisplayMode::XY {
                ui.heading("XY Mode");
                ui.separator();
                ui.label("Ch1 → X, Ch2 → Y");
                ui.horizontal(|ui| {
                    ui.label("Trail:");
                    ui.add(egui::Slider::new(&mut self.xy_trail_length, 500..=100_000).logarithmic(true));
                });
            } else {
                ui.heading("Timebase");
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("-").clicked() { self.timebase.zoom_in(); }
                    ui.label(format_time(self.timebase.time_per_div));
                    if ui.button("+").clicked() { self.timebase.zoom_out(); }
                });
                ui.horizontal(|ui| {
                    ui.label("Pos:");
                    let mut pos = self.timebase.horizontal_position as f32;
                    ui.add(egui::DragValue::new(&mut pos).speed(0.001));
                    self.timebase.horizontal_position = pos as f64;
                });
            }

            if self.show_cursors {
                ui.separator();
                ui.heading("Cursors");
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.cursors.time.enabled, "Time");
                });
                if self.cursors.time.enabled {
                    let mut a = self.cursors.time.a as f32;
                    let mut b = self.cursors.time.b as f32;
                    ui.horizontal(|ui| {
                        ui.label("A:"); ui.add(egui::DragValue::new(&mut a).speed(0.0001));
                        ui.label("B:"); ui.add(egui::DragValue::new(&mut b).speed(0.0001));
                    });
                    self.cursors.time.a = a as f64;
                    self.cursors.time.b = b as f64;
                    ui.label(format!("dt: {}", format_time(self.cursors.time.delta())));
                    if let Some(f) = self.cursors.time_delta_frequency() {
                        ui.label(format!("1/dt: {:.2} Hz", f));
                    }
                }
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.cursors.voltage.enabled, "Voltage");
                });
                if self.cursors.voltage.enabled {
                    let mut a = self.cursors.voltage.a as f32;
                    let mut b = self.cursors.voltage.b as f32;
                    ui.horizontal(|ui| {
                        ui.label("A:"); ui.add(egui::DragValue::new(&mut a).speed(0.01));
                        ui.label("B:"); ui.add(egui::DragValue::new(&mut b).speed(0.01));
                    });
                    self.cursors.voltage.a = a as f64;
                    self.cursors.voltage.b = b as f64;
                    ui.label(format!("dV: {:.4}", self.cursors.voltage.delta()));
                }
            }

            if self.show_measurements {
                ui.separator();
                ui.heading("Measurements");
                for (i, m) in self.measurements.iter().enumerate() {
                    if i >= self.channels.len() || !self.channels[i].enabled {
                        continue;
                    }
                    let color = &self.channels[i].color;
                    let c32 = egui::Color32::from_rgb(color.r, color.g, color.b);
                    ui.colored_label(c32, &self.channels[i].label);
                    ui.label(format!("  Vpp: {:.4}  Vrms: {:.4}", m.vpp, m.vrms));
                    ui.label(format!("  Min: {:.4}  Max: {:.4}", m.min, m.max));
                    if let Some(f) = m.frequency {
                        ui.label(format!("  Freq: {:.2} Hz", f));
                    }
                }
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let available = ui.available_size();
            let (response, painter) =
                ui.allocate_painter(available, egui::Sense::hover());
            let rect = response.rect;

            match self.display_mode {
                DisplayMode::XY => {
                    draw_xy(&painter, rect, &self.channels, self.xy_trail_length);
                }
                DisplayMode::YT => {
                    let buf_len = if !self.channels.is_empty() {
                        self.channels[0].buffer.len()
                    } else {
                        0
                    };
                    let display_start = self.display_window_start(buf_len);

                    let trigger_at_count = match &self.sweep {
                        SweepState::Displaying { trigger_at, .. } => Some(*trigger_at),
                        _ => None,
                    };
                    let trigger_buf_idx = trigger_at_count.and_then(|tc| {
                        let ago = self.total_samples.saturating_sub(tc) as usize;
                        if ago < buf_len { Some(buf_len - 1 - ago) } else { None }
                    });

                    draw_scope(
                        &painter, rect, &self.channels, &self.timebase, &self.cursors,
                        display_start, trigger_buf_idx,
                    );
                }
            }
        });
    }
}

fn draw_xy(
    painter: &egui::Painter,
    rect: egui::Rect,
    channels: &[Channel],
    trail_length: usize,
) {
    let bg = egui::Color32::from_rgb(10, 10, 20);
    painter.rect_filled(rect, 0.0, bg);

    if channels.len() < 2 || channels[0].buffer.is_empty() || channels[1].buffer.is_empty() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "XY mode requires 2 channels",
            egui::FontId::monospace(14.0),
            egui::Color32::from_rgb(150, 150, 150),
        );
        return;
    }

    let ch_x = &channels[0];
    let ch_y = &channels[1];

    let n = ch_x.buffer.len().min(ch_y.buffer.len());
    let trail = trail_length.min(n);
    if trail < 2 {
        return;
    }
    let start = n - trail;

    // Square plotting area centered in the rect with 5% margin
    let side = rect.width().min(rect.height()) * 0.95;
    let plot_rect = egui::Rect::from_center_size(
        rect.center(),
        egui::vec2(side, side),
    );

    // Draw graticule (8x8 grid)
    let divs = 8u32;
    let div_size = side / divs as f32;
    let grid_color = egui::Color32::from_rgba_premultiplied(50, 50, 50, 255);
    let grid_center = egui::Color32::from_rgba_premultiplied(70, 70, 70, 255);

    for i in 0..=divs {
        let f = i as f32 * div_size;
        let color = if i * 2 == divs { grid_center } else { grid_color };
        painter.line_segment(
            [egui::pos2(plot_rect.left() + f, plot_rect.top()),
             egui::pos2(plot_rect.left() + f, plot_rect.bottom())],
            egui::Stroke::new(1.0, color),
        );
        painter.line_segment(
            [egui::pos2(plot_rect.left(), plot_rect.top() + f),
             egui::pos2(plot_rect.right(), plot_rect.top() + f)],
            egui::Stroke::new(1.0, color),
        );
    }

    // Auto-scale: scan the trail to find data range
    let mut x_min = f32::MAX;
    let mut x_max = f32::MIN;
    let mut y_min = f32::MAX;
    let mut y_max = f32::MIN;

    let scan_step = (trail / 2000).max(1);
    let mut si = 0;
    while si < trail {
        let idx = start + si;
        if let Some(vx) = ch_x.buffer.get(idx) {
            x_min = x_min.min(vx);
            x_max = x_max.max(vx);
        }
        if let Some(vy) = ch_y.buffer.get(idx) {
            y_min = y_min.min(vy);
            y_max = y_max.max(vy);
        }
        si += scan_step;
    }

    let x_range = (x_max - x_min).max(1e-6);
    let y_range = (y_max - y_min).max(1e-6);
    let x_center = (x_min + x_max) / 2.0;
    let y_center = (y_min + y_max) / 2.0;
    // Use the larger range for both axes so the aspect ratio is 1:1
    let data_range = x_range.max(y_range) * 1.1; // 10% padding
    let half_range = data_range / 2.0;

    let x_to_pixel = |val: f32| -> f32 {
        let frac = (val - x_center) / half_range; // -1..1
        plot_rect.center().x + frac * (side / 2.0)
    };
    let y_to_pixel = |val: f32| -> f32 {
        let frac = (val - y_center) / half_range;
        plot_rect.center().y - frac * (side / 2.0) // Y up
    };

    // Build point list with phosphor fade (older = dimmer)
    let pixels_budget = (side as usize) * 4;
    let step = (trail / pixels_budget).max(1);
    let mut points: Vec<(egui::Pos2, f32)> = Vec::with_capacity(pixels_budget + 1);

    let mut i = 0;
    while i < trail {
        let idx = start + i;
        if let (Some(vx), Some(vy)) = (ch_x.buffer.get(idx), ch_y.buffer.get(idx)) {
            let px = x_to_pixel(vx).clamp(plot_rect.left(), plot_rect.right());
            let py = y_to_pixel(vy).clamp(plot_rect.top(), plot_rect.bottom());
            let age_frac = i as f32 / trail as f32;
            points.push((egui::pos2(px, py), age_frac));
        }
        i += step;
    }

    // Draw the trace with phosphor glow
    let base_color = egui::Color32::from_rgb(0, 255, 100);
    if points.len() >= 2 {
        for w in points.windows(2) {
            let (p0, age0) = w[0];
            let (p1, age1) = w[1];
            let avg_age = (age0 + age1) / 2.0;
            let alpha = (avg_age * 220.0 + 35.0) as u8;
            let color = egui::Color32::from_rgba_unmultiplied(
                base_color.r(),
                base_color.g(),
                base_color.b(),
                alpha,
            );
            let width = 0.5 + 1.5 * avg_age;
            painter.line_segment([p0, p1], egui::Stroke::new(width, color));
        }
    }

    // Axis range labels
    let label_color = egui::Color32::from_rgb(150, 150, 150);
    let font = egui::FontId::monospace(10.0);
    let range_per_div = data_range / divs as f32;
    painter.text(
        egui::pos2(plot_rect.right() - 2.0, plot_rect.center().y + 2.0),
        egui::Align2::RIGHT_TOP,
        format!("X: {:.1}/div", range_per_div),
        font.clone(),
        label_color,
    );
    painter.text(
        egui::pos2(plot_rect.center().x + 2.0, plot_rect.top() + 2.0),
        egui::Align2::LEFT_TOP,
        format!("Y: {:.1}/div", range_per_div),
        font.clone(),
        label_color,
    );
    painter.text(
        egui::pos2(plot_rect.left() + 2.0, plot_rect.bottom() - 2.0),
        egui::Align2::LEFT_BOTTOM,
        format!("X[{:.1}..{:.1}] Y[{:.1}..{:.1}]", x_min, x_max, y_min, y_max),
        font,
        label_color,
    );
}

fn draw_scope(
    painter: &egui::Painter,
    rect: egui::Rect,
    channels: &[Channel],
    timebase: &Timebase,
    cursors: &Cursors,
    display_start: usize,
    trigger_buf_idx: Option<usize>,
) {
    let bg = egui::Color32::from_rgb(10, 10, 20);
    painter.rect_filled(rect, 0.0, bg);

    let h_divs = timebase.num_horizontal_divs as f32;
    let v_divs = 8.0f32;
    let div_w = rect.width() / h_divs;
    let div_h = rect.height() / v_divs;

    // Graticule grid
    let grid_color = egui::Color32::from_rgba_premultiplied(60, 60, 60, 255);
    let grid_center = egui::Color32::from_rgba_premultiplied(80, 80, 80, 255);

    for i in 0..=timebase.num_horizontal_divs {
        let x = rect.left() + i as f32 * div_w;
        let color = if i * 2 == timebase.num_horizontal_divs { grid_center } else { grid_color };
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            egui::Stroke::new(1.0, color),
        );
    }
    for i in 0..=v_divs as u32 {
        let y = rect.top() + i as f32 * div_h;
        let color = if i * 2 == v_divs as u32 { grid_center } else { grid_color };
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            egui::Stroke::new(1.0, color),
        );
    }

    // Tick marks on center cross
    let cx = rect.center().x;
    let cy = rect.center().y;
    let tick_len = 4.0;
    let tick_color = egui::Color32::from_rgb(100, 100, 100);
    for i in 0..=(h_divs as u32 * 5) {
        let x = rect.left() + i as f32 * div_w / 5.0;
        painter.line_segment(
            [egui::pos2(x, cy - tick_len), egui::pos2(x, cy + tick_len)],
            egui::Stroke::new(1.0, tick_color),
        );
    }
    for i in 0..=(v_divs as u32 * 5) {
        let y = rect.top() + i as f32 * div_h / 5.0;
        painter.line_segment(
            [egui::pos2(cx - tick_len, y), egui::pos2(cx + tick_len, y)],
            egui::Stroke::new(1.0, tick_color),
        );
    }

    // Draw trigger marker at the trigger point's x position
    if let Some(trig_idx) = trigger_buf_idx {
        let vis = timebase.visible_samples();
        if trig_idx >= display_start && trig_idx < display_start + vis {
            let frac = (trig_idx - display_start) as f32 / vis as f32;
            let x = rect.left() + frac * rect.width();
            let trig_color = egui::Color32::from_rgb(255, 120, 0);
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.top() + 12.0)],
                egui::Stroke::new(2.0, trig_color),
            );
            painter.text(
                egui::pos2(x, rect.top() + 1.0),
                egui::Align2::CENTER_TOP,
                "T",
                egui::FontId::monospace(9.0),
                trig_color,
            );
        }
    }

    // Draw traces
    for ch in channels {
        if !ch.enabled || ch.buffer.is_empty() {
            continue;
        }
        let color = egui::Color32::from_rgb(ch.color.r, ch.color.g, ch.color.b);
        let start = display_start.min(ch.buffer.len());
        let vis = timebase.visible_samples().min(ch.buffer.len().saturating_sub(start));
        if vis < 2 {
            continue;
        }

        let pixels_available = rect.width() as usize;
        let step = (vis / pixels_available).max(1);

        let mut points = Vec::with_capacity(pixels_available + 1);
        let mut i = 0;
        while i < vis {
            if let Some(val) = ch.buffer.get(start + i) {
                let divs_y = ch.value_to_divs(val);
                let x = rect.left() + (i as f32 / vis as f32) * rect.width();
                let y = rect.center().y - divs_y * div_h;
                let y_clamped = y.clamp(rect.top(), rect.bottom());
                points.push(egui::pos2(x, y_clamped));
            }
            i += step;
        }

        if points.len() >= 2 {
            let stroke = egui::Stroke::new(1.5, color);
            for w in points.windows(2) {
                painter.line_segment([w[0], w[1]], stroke);
            }
        }
    }

    // Draw cursor lines
    if cursors.time.enabled {
        let cursor_color = egui::Color32::from_rgba_premultiplied(200, 200, 200, 150);
        for &t in &[cursors.time.a, cursors.time.b] {
            let frac = t as f32 / (timebase.time_per_div as f32 * h_divs);
            let x = rect.left() + frac * rect.width();
            if x >= rect.left() && x <= rect.right() {
                painter.line_segment(
                    [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                    egui::Stroke::new(1.0, cursor_color),
                );
            }
        }
    }
    if cursors.voltage.enabled {
        let cursor_color = egui::Color32::from_rgba_premultiplied(200, 200, 200, 150);
        for &v in &[cursors.voltage.a, cursors.voltage.b] {
            let y = rect.center().y - v as f32 * div_h;
            if y >= rect.top() && y <= rect.bottom() {
                painter.line_segment(
                    [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                    egui::Stroke::new(1.0, cursor_color),
                );
            }
        }
    }

    // Division labels
    let label_color = egui::Color32::from_rgb(150, 150, 150);
    let font = egui::FontId::monospace(10.0);
    // Bottom row: time labels
    for i in 0..=timebase.num_horizontal_divs {
        let x = rect.left() + i as f32 * div_w;
        let t = timebase.div_to_time(i as f64);
        painter.text(
            egui::pos2(x + 2.0, rect.bottom() - 12.0),
            egui::Align2::LEFT_BOTTOM,
            format_time(t),
            font.clone(),
            label_color,
        );
    }
}

fn format_time(t: f64) -> String {
    let abs = t.abs();
    if abs < 1e-6 {
        format!("{:.1}ns", t * 1e9)
    } else if abs < 1e-3 {
        format!("{:.1}us", t * 1e6)
    } else if abs < 1.0 {
        format!("{:.1}ms", t * 1e3)
    } else {
        format!("{:.2}s", t)
    }
}

fn main() {
    let cli = Cli::parse();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 700.0])
            .with_title("kleis-scope"),
        ..Default::default()
    };

    let _ = eframe::run_native(
        "kleis-scope",
        options,
        Box::new(move |_cc| Ok(Box::new(ScopeApp::new(cli)))),
    );
}
