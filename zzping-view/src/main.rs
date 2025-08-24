use eframe::egui;
use std::path::PathBuf;
use clap::Parser;
use chrono::Duration;

mod data;
mod plot;
use data::PingData;

/// A simple viewer for zzping data.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    /// The path to the compressed input data file to be visualized.
    #[arg()]
    input_file: PathBuf,
}

struct ZzpingViewApp {
    data: PingData,
    /// Pan offset, in microseconds.
    pan_micros: i64,
    zoom: f32, // 1.0 (full view) up to max_zoom
}

impl ZzpingViewApp {
    fn new(data: PingData) -> Self {
        Self {
            data,
            pan_micros: 0,
            zoom: 1.0,
        }
    }
}

impl eframe::App for ZzpingViewApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::bottom("controls")
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Calculate the valid range for the pan slider.
                    let full_time_range = if let (Some(first), Some(last)) = (self.data.points.first(), self.data.points.last()) {
                        last.time - first.time
                    } else {
                        Duration::zero()
                    };

                    let view_duration = full_time_range / self.zoom as i32;
                    let max_pan = full_time_range - view_duration;
                    let max_pan_micros = max_pan.num_microseconds().unwrap_or(0);

                    // Clamp pan to the new valid range
                    self.pan_micros = self.pan_micros.min(max_pan_micros);

                    ui.label("Pan:");
                    ui.add(egui::Slider::new(&mut self.pan_micros, 0..=max_pan_micros).text("μs"));

                    ui.label("Zoom:");
                    // Using a logarithmic scale for zoom feels more natural.
                    ui.add(egui::Slider::new(&mut self.zoom, 1.0..=1000.0).logarithmic(true));
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            let widget = plot::PlotWidget::new(&self.data.points, self.pan_micros, self.zoom);
            ui.add(widget);
        });
    }
}


fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let raw_data = std::fs::read(&cli.input_file)?;
    let ping_data = data::load_and_parse(&raw_data)?;

    let options = eframe::NativeOptions {
        initial_window_size: Some(egui::vec2(1280.0, 720.0)),
        ..Default::default()
    };

    eframe::run_native(
        "zzping-view",
        options,
        Box::new(|_cc| Box::new(ZzpingViewApp::new(ping_data))),
    ).map_err(|e| anyhow::anyhow!(e.to_string()))?;

    Ok(())
}
