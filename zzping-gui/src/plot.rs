//! A custom `egui` widget for visualizing ping data.
//!
//! This module contains `PlotWidget`, a highly optimized widget for displaying
//! potentially large time-series datasets of ping RTTs. It includes logic for
//! panning, zooming, and dynamically switching between a high-level "min/max bar"
//! view and a detailed "point" view based on data density.

use crate::data::DataPoint;
use chrono::{DateTime, Duration, Utc};
use egui::{Painter, Pos2, Rect, Response, Sense, Stroke, Ui, Widget, emath::RectTransform};

/// The maximum RTT to display on the plot's Y-axis, in milliseconds.
///
/// Any RTT value higher than this will be clamped. This prevents extreme outliers
/// from squashing the rest of the data and making the plot unreadable.
const MAX_RTT_CLAMP_MS: f32 = 500.0;

/// An `egui` widget that renders the ping data plot.
///
/// This widget is responsible for all the custom drawing logic for the time-series
/// data, including the grid, axis labels, and the data points themselves.
pub struct PlotWidget<'a> {
    /// A slice of the `DataPoint`s to be rendered.
    points: &'a [DataPoint],
    /// The current horizontal pan offset, in microseconds from the start of the dataset.
    pan_micros: i64,
    /// The current zoom level. 1.0 is fully zoomed out.
    zoom: f32,
}

impl<'a> PlotWidget<'a> {
    /// Creates a new `PlotWidget`.
    pub fn new(points: &'a [DataPoint], pan_micros: i64, zoom: f32) -> Self {
        Self {
            points,
            pan_micros,
            zoom,
        }
    }
}

impl<'a> Widget for PlotWidget<'a> {
    fn ui(self, ui: &mut Ui) -> Response {
        // Allocate the space for the plot.
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
        let painter = ui.painter_at(rect);

        painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(20, 20, 20));

        if self.points.is_empty() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Waiting for data from database...",
                egui::FontId::proportional(20.0),
                egui::Color32::GRAY,
            );
            return response;
        }

        // 1. Calculate data ranges
        let (full_time_range, max_rtt_ms) = self.get_data_ranges();
        let view_duration_nanos = (full_time_range.num_nanoseconds().unwrap_or(0) as f64
            / self.zoom as f64)
            .max(1_000_000_000.0);
        let view_duration = Duration::nanoseconds(view_duration_nanos as i64);
        let view_start_time = self.points[0].time + Duration::microseconds(self.pan_micros);
        let view_end_time = view_start_time + view_duration;

        // 2. Set up coordinate transformation
        let view_duration_millis = view_duration.num_milliseconds() as f32;
        if view_duration_millis <= 0.0 || max_rtt_ms <= 0.0 {
            return response; // Avoid division by zero or invalid ranges
        }
        let data_rect = Rect::from_x_y_ranges(0.0..=view_duration_millis, max_rtt_ms..=0.0);
        let to_screen = RectTransform::from_to(data_rect, rect);

        // 3. Decide on rendering method based on data density
        let visible_points_count = self
            .points
            .iter()
            .filter(|p| p.time >= view_start_time && p.time <= view_end_time)
            .count();
        let use_point_rendering = (visible_points_count as f32) < rect.width();

        if use_point_rendering {
            self.draw_points(&painter, to_screen, view_start_time, view_end_time);
        } else {
            self.draw_min_max_bars(&painter, to_screen, rect, view_start_time);
        }

        // 4. Draw axis labels and grid
        self.draw_axis_labels(
            &painter,
            rect,
            &to_screen,
            max_rtt_ms,
            view_start_time,
            view_end_time,
        );

        response
    }
}

impl<'a> PlotWidget<'a> {
    /// Calculates the total time range and the maximum RTT of the dataset.
    fn get_data_ranges(&self) -> (Duration, f32) {
        let first_time = self.points.first().map(|p| p.time).unwrap_or_default();
        let last_time = self.points.last().map(|p| p.time).unwrap_or_default();
        let full_time_range = last_time - first_time;

        let max_rtt_ms = self
            .points
            .iter()
            .filter_map(|p| p.rtt)
            .map(|rtt| rtt.num_nanoseconds().unwrap_or(0) as f64 / 1_000_000.0)
            .fold(0.0f64, f64::max)
            .min(MAX_RTT_CLAMP_MS as f64) as f32;

        (full_time_range, max_rtt_ms)
    }

    /// Renders the data as individual points.
    /// Used when the data density is low enough to see individual pings.
    fn draw_points(
        &self,
        painter: &Painter,
        to_screen: RectTransform,
        view_start: DateTime<Utc>,
        view_end: DateTime<Utc>,
    ) {
        let visible_points = self
            .points
            .iter()
            .filter(|p| p.time >= view_start && p.time <= view_end);
        let success_color = egui::Color32::from_rgb(100, 200, 255);
        let lost_color = egui::Color32::from_rgb(255, 100, 100);

        for p in visible_points {
            let relative_time_ms = (p.time - view_start).num_milliseconds() as f32;
            if let Some(rtt) = p.rtt {
                let rtt_ms = rtt.num_nanoseconds().unwrap_or(0) as f64 / 1_000_000.0;
                let screen_pos = to_screen * Pos2::new(relative_time_ms, rtt_ms as f32);
                painter.circle_filled(screen_pos, 2.0, success_color);
            } else {
                let screen_pos = to_screen * Pos2::new(relative_time_ms, 0.0);
                painter.rect_filled(
                    Rect::from_center_size(screen_pos, egui::Vec2::new(2.0, 8.0)),
                    0.0,
                    lost_color,
                );
            }
        }
    }

    /// Renders the data as vertical bars representing the min/max RTT in a given time slice.
    /// Used when the data density is too high to render individual points.
    fn draw_min_max_bars(
        &self,
        painter: &Painter,
        to_screen: RectTransform,
        rect: Rect,
        view_start_time: DateTime<Utc>,
    ) {
        let bar_color = egui::Color32::from_rgb(100, 200, 255);
        let stroke = Stroke::new(1.0, bar_color);
        let from_screen = to_screen.inverse();

        let step_size = 0.5;
        let mut screen_x = rect.min.x;

        while screen_x <= rect.max.x {
            let t_start_relative = from_screen.transform_pos(Pos2::new(screen_x, 0.0)).x as i64;
            let t_end_relative = from_screen
                .transform_pos(Pos2::new(screen_x + step_size, 0.0))
                .x as i64;

            let t_start = view_start_time + Duration::milliseconds(t_start_relative);
            let t_end = view_start_time + Duration::milliseconds(t_end_relative);

            let start_idx = self.points.partition_point(|p| p.time < t_start);
            let end_idx = self.points.partition_point(|p| p.time < t_end);

            if start_idx < end_idx {
                let point_slice = &self.points[start_idx..end_idx];
                let mut slice_min_rtt_ms = f64::INFINITY;
                let mut slice_max_rtt_ms = 0.0f64;
                let mut success_count = 0;
                let mut lost_count = 0;

                for p in point_slice {
                    if let Some(rtt) = p.rtt {
                        let rtt_ms = rtt.num_nanoseconds().unwrap_or(0) as f64 / 1_000_000.0;
                        slice_min_rtt_ms = slice_min_rtt_ms.min(rtt_ms);
                        slice_max_rtt_ms = slice_max_rtt_ms.max(rtt_ms);
                        success_count += 1;
                    } else {
                        lost_count += 1;
                    }
                }

                if success_count > 0 {
                    let y_high = to_screen
                        .transform_pos(Pos2::new(0.0, slice_min_rtt_ms as f32))
                        .y;
                    let y_low = to_screen
                        .transform_pos(Pos2::new(0.0, slice_max_rtt_ms as f32))
                        .y;
                    painter.line_segment(
                        [Pos2::new(screen_x, y_high), Pos2::new(screen_x, y_low)],
                        stroke,
                    );
                }

                if lost_count > 0 {
                    let y_bottom = to_screen.transform_pos(Pos2::new(0.0, 0.0)).y;
                    let loss_intensity =
                        (lost_count as f32 / (success_count + lost_count) as f32).min(1.0);
                    let alpha = (255.0 * loss_intensity) as u8;
                    let loss_color_alpha =
                        egui::Color32::from_rgba_unmultiplied(255, 100, 100, alpha);
                    painter.line_segment(
                        [
                            Pos2::new(screen_x, y_bottom),
                            Pos2::new(screen_x, y_bottom - 4.0),
                        ],
                        Stroke::new(1.0, loss_color_alpha),
                    );
                }
            }
            screen_x += step_size;
        }
    }

    /// Draws the X and Y axis labels and grid lines.
    fn draw_axis_labels(
        &self,
        painter: &Painter,
        rect: Rect,
        to_screen: &RectTransform,
        max_rtt_ms: f32,
        view_start_time: DateTime<Utc>,
        view_end_time: DateTime<Utc>,
    ) {
        let text_color = egui::Color32::from_rgb(180, 180, 180);
        let grid_color = egui::Color32::from_rgb(50, 50, 50);
        let font_size = 12.0;

        // Draw Y-axis (RTT) labels
        let num_y_ticks = 5;
        for i in 0..=num_y_ticks {
            let rtt_ms = (max_rtt_ms * i as f32) / num_y_ticks as f32;
            let y_pos = to_screen.transform_pos(Pos2::new(0.0, rtt_ms)).y;
            if y_pos >= rect.min.y && y_pos <= rect.max.y {
                painter.line_segment(
                    [Pos2::new(rect.min.x, y_pos), Pos2::new(rect.max.x, y_pos)],
                    Stroke::new(1.0, grid_color),
                );
                let label = if rtt_ms >= 1000.0 {
                    format!("{:.1}s", rtt_ms / 1000.0)
                } else {
                    format!("{rtt_ms:.1}ms")
                };
                painter.text(
                    Pos2::new(rect.min.x + 5.0, y_pos - 8.0),
                    egui::Align2::LEFT_CENTER,
                    label,
                    egui::FontId::proportional(font_size),
                    text_color,
                );
            }
        }

        // Draw X-axis (Time) labels
        let view_duration_millis = (view_end_time - view_start_time).num_milliseconds() as f32;
        let num_x_ticks = 6;
        for i in 0..=num_x_ticks {
            let time_progress = i as f32 / num_x_ticks as f32;
            let time_millis =
                view_start_time.timestamp_millis() as f32 + (view_duration_millis * time_progress);
            let x_pos = to_screen.transform_pos(Pos2::new(time_millis, 0.0)).x;

            if x_pos >= rect.min.x && x_pos <= rect.max.x {
                painter.line_segment(
                    [Pos2::new(x_pos, rect.min.y), Pos2::new(x_pos, rect.max.y)],
                    Stroke::new(1.0, grid_color),
                );
                let actual_time = view_start_time
                    + Duration::milliseconds((view_duration_millis * time_progress) as i64);
                let time_label = if view_duration_millis > 3_600_000.0 {
                    actual_time.format("%H:%M").to_string()
                } else if view_duration_millis > 60_000.0 {
                    actual_time.format("%M:%S").to_string()
                } else {
                    let secs = actual_time.format("%S").to_string();
                    let millis = actual_time.timestamp_millis() % 1000;
                    format!("{secs}.{millis:03}s")
                };
                painter.text(
                    Pos2::new(x_pos, rect.max.y - 15.0),
                    egui::Align2::CENTER_CENTER,
                    time_label,
                    egui::FontId::proportional(font_size),
                    text_color,
                );
            }
        }

        // Draw title
        painter.text(
            Pos2::new(rect.center().x, rect.min.y + 15.0),
            egui::Align2::CENTER_CENTER,
            "Ping RTT over Time",
            egui::FontId::proportional(16.0),
            egui::Color32::WHITE,
        );
    }
}
