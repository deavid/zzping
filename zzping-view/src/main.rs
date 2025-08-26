use chrono::Duration;
use clap::Parser;
use eframe::egui;
use std::path::PathBuf;

mod data;
mod plot;
use data::PingData;

/// A viewer for zzping-capture data files.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    /// The path to the zzping-capture data file (.dat) to be visualized.
    #[arg()]
    input_file: PathBuf,

    /// Limit the number of data points loaded for faster debugging (default: no limit)
    #[arg(long, short = 'l')]
    limit: Option<usize>,
}

struct ZzpingViewApp {
    data: PingData,
    /// Pan offset, in microseconds (internal representation)
    pan_micros: i64,
    /// Zoom level: 1.0 shows full view, higher values zoom in
    zoom: f32,
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
        egui::TopBottomPanel::bottom("controls").show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Calculate the valid range for the pan slider based on the data time range
                let full_time_range = if let (Some(first), Some(last)) =
                    (self.data.points.first(), self.data.points.last())
                {
                    last.time - first.time
                } else {
                    Duration::zero()
                };

                let view_duration = full_time_range / self.zoom as i32;
                let max_pan = full_time_range - view_duration;
                let max_pan_micros = max_pan.num_microseconds().unwrap_or(0);

                // Clamp pan to the valid range
                self.pan_micros = self.pan_micros.min(max_pan_micros).max(0);

                // Convert pan to human-readable format
                let pan_duration = Duration::microseconds(self.pan_micros);
                let hours = pan_duration.num_hours();
                let mins = pan_duration.num_minutes() % 60;
                let secs = pan_duration.num_seconds() % 60;
                let millis = pan_duration.num_milliseconds() % 1000;
                let pan_display = format!("{}h {}min {}.{:03}s", hours, mins, secs, millis);

                ui.label("Pan:");
                ui.label(pan_display);

                // Add a slider that shows percentage of dataset
                let pan_percentage = if max_pan_micros > 0 {
                    (self.pan_micros as f32 / max_pan_micros as f32) * 100.0
                } else {
                    0.0
                };
                let mut pan_percent = pan_percentage;
                ui.add(
                    egui::Slider::new(&mut pan_percent, 0.0..=100.0)
                        .text("%")
                        .show_value(false),
                );

                // Convert percentage back to microseconds
                self.pan_micros = ((pan_percent / 100.0) * max_pan_micros as f32) as i64;

                ui.label("Zoom:");
                // Calculate maximum useful zoom based on data size and time span
                // Prevent zoom levels that would result in sub-millisecond view windows
                let max_useful_zoom = if !self.data.points.is_empty() {
                    let time_span_ms = (self.data.points.last().unwrap().time
                        - self.data.points[0].time)
                        .num_milliseconds() as f32;
                    // Minimum view window of 10 milliseconds
                    (time_span_ms / 10.0).clamp(1.0, 100.0)
                } else {
                    100.0
                };
                ui.add(egui::Slider::new(&mut self.zoom, 1.0..=max_useful_zoom).logarithmic(true));

                // Add some info about the dataset
                ui.separator();
                ui.label(format!("Points: {}", self.data.points.len()));
                let lost_count = self.data.points.iter().filter(|p| p.rtt.is_none()).count();
                ui.label(format!("Lost: {}", lost_count));

                // Show zoom level as a percentage of full dataset
                ui.label(format!("Zoom: {:.1}x", self.zoom));
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

    eprintln!(
        "Loading zzping-capture data from: {}",
        cli.input_file.display()
    );
    let raw_data = std::fs::read(&cli.input_file)?;
    let ping_data = data::load_and_parse_with_limit(&raw_data, cli.limit)?;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 720.0])
            .with_title("zzping-view - Ping Data Visualizer"),
        ..Default::default()
    };

    eframe::run_native(
        "zzping-view",
        options,
        Box::new(|_cc| Ok(Box::new(ZzpingViewApp::new(ping_data)))),
    )
    .map_err(|e| anyhow::anyhow!("Failed to run GUI application: {}", e))?;

    Ok(())
}
