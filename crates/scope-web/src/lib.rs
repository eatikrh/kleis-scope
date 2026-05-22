use wasm_bindgen::prelude::*;

/// Entry point for the WASM build. Mounts the egui app into the given canvas ID.
#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();

    let web_options = eframe::WebOptions::default();
    wasm_bindgen_futures::spawn_local(async {
        let _ = eframe::WebRunner::new()
            .start(
                "scope_canvas",
                web_options,
                Box::new(|_cc| Ok(Box::new(ScopeWebApp::new()))),
            )
            .await;
    });

    Ok(())
}

use scope_core::channel::Channel;
use scope_core::cursor::Cursors;
use scope_core::measure::Measurements;
use scope_core::timebase::Timebase;
use scope_core::trigger::{TriggerEngine, TriggerMode, TriggerEdge};

/// Minimal web-specific app state. Data arrives via postMessage from the host page.
struct ScopeWebApp {
    channels: Vec<Channel>,
    timebase: Timebase,
    trigger: TriggerEngine,
    cursors: Cursors,
    measurements: Vec<Measurements>,
    show_measurements: bool,
    show_cursors: bool,
    pending_samples: Vec<Vec<f32>>,
}

impl ScopeWebApp {
    fn new() -> Self {
        let n_ch = 2;
        Self {
            channels: (0..n_ch).map(Channel::new).collect(),
            timebase: Timebase::new(10000.0),
            trigger: TriggerEngine::new(),
            cursors: Cursors::new(),
            measurements: vec![Measurements::default(); n_ch],
            show_measurements: true,
            show_cursors: false,
            pending_samples: Vec::new(),
        }
    }
}

impl eframe::App for ScopeWebApp {
    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        // In a real deployment, JavaScript would call into WASM to push samples.
        // For now this is a scaffold that renders the empty scope UI.
        ctx.request_repaint();

        eframe::egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("kleis-scope (web)");
            ui.label("Connect a data source via postMessage to see traces.");

            let available = ui.available_size();
            let (response, painter) =
                ui.allocate_painter(available, eframe::egui::Sense::hover());
            let rect = response.rect;

            // Draw background and graticule
            let bg = eframe::egui::Color32::from_rgb(10, 10, 20);
            painter.rect_filled(rect, 0.0, bg);

            let h_divs = 10.0f32;
            let v_divs = 8.0f32;
            let div_w = rect.width() / h_divs;
            let div_h = rect.height() / v_divs;
            let grid_color = eframe::egui::Color32::from_rgb(60, 60, 60);

            for i in 0..=h_divs as u32 {
                let x = rect.left() + i as f32 * div_w;
                painter.line_segment(
                    [eframe::egui::pos2(x, rect.top()), eframe::egui::pos2(x, rect.bottom())],
                    eframe::egui::Stroke::new(1.0, grid_color),
                );
            }
            for i in 0..=v_divs as u32 {
                let y = rect.top() + i as f32 * div_h;
                painter.line_segment(
                    [eframe::egui::pos2(rect.left(), y), eframe::egui::pos2(rect.right(), y)],
                    eframe::egui::Stroke::new(1.0, grid_color),
                );
            }
        });
    }
}
