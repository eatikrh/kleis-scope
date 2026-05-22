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
            if self.trigger.source_channel < self.channels.len() {
                let src_ch = self.trigger.source_channel;
                if let Some(&val) = frame.values.get(src_ch) {
                    let buf_len = self.channels[src_ch].buffer.len();
                    self.trigger.check(val, buf_len);
                }
            }
            count += 1;
            if count > 50_000 { break; }
        }
    }

    fn update_measurements(&mut self) {
        for (i, ch) in self.channels.iter().enumerate() {
            if !ch.enabled || ch.buffer.is_empty() {
                continue;
            }
            let start = self.timebase.window_start(ch.buffer.len());
            let vis = self.timebase.visible_samples().min(ch.buffer.len() - start);
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

                // Trigger controls
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

            draw_scope(&painter, rect, &self.channels, &self.timebase, &self.cursors);
        });
    }
}

fn draw_scope(
    painter: &egui::Painter,
    rect: egui::Rect,
    channels: &[Channel],
    timebase: &Timebase,
    cursors: &Cursors,
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

    // Draw traces
    for ch in channels {
        if !ch.enabled || ch.buffer.is_empty() {
            continue;
        }
        let color = egui::Color32::from_rgb(ch.color.r, ch.color.g, ch.color.b);
        let start = timebase.window_start(ch.buffer.len());
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
