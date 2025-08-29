use anyhow::Result;
use chrono::{DateTime, Duration};
use crossbeam_channel::{unbounded, Receiver};
use eframe::egui;
use zzping_common::RawDataRecord;

mod data;
mod network;
mod plot;

struct ZzpingViewApp {
    points: Vec<data::DataPoint>,
    pan_micros: i64,
    zoom: f32,
    data_rx: Receiver<Vec<RawDataRecord>>,
}

impl ZzpingViewApp {
    fn new(data_rx: Receiver<Vec<RawDataRecord>>) -> Self {
        Self {
            points: Vec::new(),
            pan_micros: 0,
            zoom: 1.0,
            data_rx,
        }
    }

    fn update_data(&mut self) {
        if let Ok(mut new_records) = self.data_rx.try_recv() {
            if !new_records.is_empty() {
                // Sort records by timestamp, as the database doesn't guarantee order
                new_records.sort_by_key(|r| r.sent_nanos);

                self.points = new_records
                    .into_iter()
                    .map(|rec| data::DataPoint {
                        time: DateTime::from_timestamp_nanos(rec.sent_nanos as i64),
                        rtt: if rec.rtt_nanos == u64::MAX {
                            None
                        } else {
                            Some(Duration::nanoseconds(rec.rtt_nanos as i64))
                        },
                    })
                    .collect();
            } else {
                // If we receive an empty vec, clear our points
                self.points.clear();
            }
        }
    }
}

impl eframe::App for ZzpingViewApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.update_data();
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

fn main() -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let _guard = rt.enter();

    let (tx, rx) = unbounded();

    std::thread::spawn(move || {
        rt.block_on(network::fetch_data_loop(tx));
    });

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 720.0])
            .with_title("zzping-gui - Live Network Monitor"),
        ..Default::default()
    };

    eframe::run_native(
        "zzping-gui",
        options,
        Box::new(|_cc| Ok(Box::new(ZzpingViewApp::new(rx)))),
    )
    .map_err(|e| anyhow::anyhow!("Failed to run GUI application: {}", e))?;

    Ok(())
}
