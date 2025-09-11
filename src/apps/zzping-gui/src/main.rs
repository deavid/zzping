//! The main entry point for the `zzping-gui` application.
//!
//! This application provides a real-time, interactive visualization of ping data
//! received from a `zzping-database` instance.
//!
//! # Architecture
//! The application is built using `egui` and `eframe`. It runs a `tokio` runtime
//! in a separate thread to handle all network communication asynchronously, ensuring
//! the UI remains responsive.
//!
//! - **`main` function**: Sets up the `tokio` runtime and the `eframe` native application.
//! - **Network Thread**: A background thread is spawned to run the `network::fetch_data_loop`,
//!   which continuously polls the database for the latest data.
//! - **`ZzpingViewApp`**: The main `eframe::App` struct. It holds the UI state and the
//!   receiving end of a channel for network data.
//! - **`update` method**: On each frame, it checks the channel for new data and, if
//!   any is present, updates its internal state, causing the plot to be redrawn.

use anyhow::Result;
use chrono::Duration;
use crossbeam_channel::{Receiver, unbounded};
use eframe::egui;
use zzping_lib::protocol::RawDataRecord;

mod data;
mod data_processing;
mod network;
mod plot;

/// The main application state for the zzping GUI.
struct ZzpingViewApp {
    /// The collection of data points currently being displayed on the plot.
    points: Vec<data::DataPoint>,
    /// The horizontal pan offset of the plot, in microseconds.
    pan_micros: i64,
    /// The current zoom level of the plot.
    zoom: f32,
    /// The receiving end of the channel for incoming data from the network thread.
    data_rx: Receiver<Vec<RawDataRecord>>,
}

impl ZzpingViewApp {
    /// Creates a new instance of the `ZzpingViewApp`.
    ///
    /// # Arguments
    /// * `data_rx` - The receiver for the channel that the network thread will use
    ///   to send data to the GUI.
    fn new(data_rx: Receiver<Vec<RawDataRecord>>) -> Self {
        Self {
            points: Vec::new(),
            pan_micros: 0,
            zoom: 1.0,
            data_rx,
        }
    }

    /// Checks for and processes new data from the network thread.
    ///
    /// This method is called on each UI frame. It performs a non-blocking check
    /// on the MPSC channel for any new data. If a new vector of records has arrived,
    /// it replaces the current `points` with the processed data.
    fn update_data(&mut self) {
        if let Ok(new_records) = self.data_rx.try_recv() {
            self.points = data_processing::records_to_points(new_records);
        }
    }
}

impl eframe::App for ZzpingViewApp {
    /// The main update method for the egui application.
    ///
    /// This is called on every frame and is responsible for drawing the entire UI.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // First, check for any new data from the network.
        self.update_data();
        // Explicitly request a repaint. This is important because the network data
        // arrives asynchronously. Without this, egui might not redraw the plot
        // immediately when new data comes in.
        ctx.request_repaint();

        egui::TopBottomPanel::bottom("controls").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let full_time_range =
                    if let (Some(first), Some(last)) = (self.points.first(), self.points.last()) {
                        last.time - first.time
                    } else {
                        Duration::zero()
                    };

                let view_duration = full_time_range / self.zoom as i32;
                let max_pan = full_time_range - view_duration;
                let max_pan_micros = max_pan.num_microseconds().unwrap_or(0);

                self.pan_micros = self.pan_micros.min(max_pan_micros).max(0);

                let pan_duration = Duration::microseconds(self.pan_micros);
                let hours = pan_duration.num_hours();
                let mins = pan_duration.num_minutes() % 60;
                let secs = pan_duration.num_seconds() % 60;
                let millis = pan_duration.num_milliseconds() % 1000;
                let pan_display = format!("{hours}h {mins}min {secs}.{millis:03}s");

                ui.label("Pan:");
                ui.label(pan_display);

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

                self.pan_micros = ((pan_percent / 100.0) * max_pan_micros as f32) as i64;

                ui.label("Zoom:");
                let max_useful_zoom = if !self.points.is_empty() {
                    let time_span_ms = (self.points.last().unwrap().time - self.points[0].time)
                        .num_milliseconds() as f32;
                    (time_span_ms / 10.0).clamp(1.0, 100.0)
                } else {
                    100.0
                };
                ui.add(egui::Slider::new(&mut self.zoom, 1.0..=max_useful_zoom).logarithmic(true));

                ui.separator();
                ui.label(format!("Points: {}", self.points.len()));
                let lost_count = self.points.iter().filter(|p| p.rtt.is_none()).count();
                ui.label(format!("Lost: {lost_count}"));

                ui.label(format!("Zoom: {:.1}x", self.zoom));
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let widget = plot::PlotWidget::new(&self.points, self.pan_micros, self.zoom);
            ui.add(widget);
        });
    }
}

/// The entry point of the application.
fn main() -> Result<()> {
    // Set up a tokio runtime for our network task.
    // We use `eframe` which has its own main loop, so we need to spawn a separate
    // thread to run the tokio runtime for our networking logic.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    // The `_guard` is important to ensure that the `tokio` runtime is active
    // when we spawn the network task.
    let _guard = rt.enter();

    // We use a `crossbeam_channel` because it's a multi-producer, multi-consumer
    // channel that is thread-safe without requiring an async runtime on the receiving end.
    // This makes it ideal for sending data from a `tokio` thread to the `egui` UI thread.
    let (tx, rx) = unbounded();

    // Spawn the network task in a separate OS thread.
    std::thread::spawn(move || {
        rt.block_on(network::fetch_data_loop(tx));
    });

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 720.0])
            .with_title("zzping-gui - Live Network Monitor"),
        ..Default::default()
    };

    // This call will block the main thread and run the `eframe` event loop.
    eframe::run_native(
        "zzping-gui",
        options,
        Box::new(|_cc| Ok(Box::new(ZzpingViewApp::new(rx)))),
    )
    .map_err(|e| anyhow::anyhow!("Failed to run GUI application: {}", e))?;

    Ok(())
}
