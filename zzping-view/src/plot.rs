use crate::data::DataPoint;
use chrono::{DateTime, Duration, Utc};
use egui::{Painter, Pos2, Rect, Response, Sense, Stroke, Ui, Widget, emath::RectTransform};

// Maximum RTT to display on the plot (in milliseconds) - anything higher gets clamped for better visualization
const MAX_RTT_CLAMP_MS: f32 = 500.0; // 500ms should cover most reasonable RTT values

pub struct PlotWidget<'a> {
    points: &'a [DataPoint],
    pan_micros: i64,
    zoom: f32,
}

impl<'a> PlotWidget<'a> {
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
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
        let painter = ui.painter_at(rect);

        painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(20, 20, 20));

        // Early exit to reduce debug spam - only print when zoom/pan changes or every 60 frames
        let should_debug = false; // Disable debug output for normal operation

        if should_debug {
            // DEBUG: Validate entry point inputs
            eprintln!("=== ENTRY POINT DEBUG ===");
            eprintln!("Total points received: {}", self.points.len());
            eprintln!("Pan micros: {}", self.pan_micros);
            eprintln!("Zoom level: {}", self.zoom);
            eprintln!("UI rect size: {:?}", rect.size());

            if !self.points.is_empty() {
                eprintln!("First point time: {:?}", self.points[0].time);
                eprintln!("Last point time: {:?}", self.points.last().unwrap().time);
                let with_rtt = self.points.iter().filter(|p| p.rtt.is_some()).count();
                eprintln!("Points with RTT: {} / {}", with_rtt, self.points.len());
            }
        }

        if self.points.is_empty() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "No data points",
                egui::FontId::proportional(20.0),
                egui::Color32::GRAY,
            );
            return response;
        }

        // 1. Calculate data ranges
        let (full_time_range, max_rtt_ms) = self.get_data_ranges();

        // Calculate view duration with floating point precision to prevent truncation
        // Minimum view duration is 1 second to prevent zero-width time ranges
        let view_duration_nanos = (full_time_range.num_nanoseconds().unwrap_or(0) as f64
            / self.zoom as f64)
            .max(1_000_000_000.0);
        let view_duration = Duration::nanoseconds(view_duration_nanos as i64);

        let view_start_time = self.points[0].time + Duration::microseconds(self.pan_micros);
        let view_end_time = view_start_time + view_duration;

        if should_debug {
            eprintln!("=== TIME CALCULATION DEBUG ===");
            eprintln!("Full time range: {} seconds", full_time_range.num_seconds());
            eprintln!("View duration nanos: {}", view_duration_nanos);
            eprintln!("View duration: {} seconds", view_duration.num_seconds());
            eprintln!("View start time: {}", view_start_time);
            eprintln!("View end time: {}", view_end_time);
            eprintln!("Start millis: {}", view_start_time.timestamp_millis());
            eprintln!("End millis: {}", view_end_time.timestamp_millis());
            eprintln!("Max RTT ms: {}", max_rtt_ms);
        }

        // Calculate duration in milliseconds for better precision
        let view_duration_millis = view_duration.num_milliseconds() as f64;

        // SAFETY CHECK: Prevent zero-width ranges that crash RectTransform
        if view_duration_millis <= 0.0 {
            // Draw error message instead of crashing
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                format!(
                    "Zoom too high ({:.1}x)\nView duration: {:.3}ms",
                    self.zoom, view_duration_millis
                ),
                egui::FontId::proportional(16.0),
                egui::Color32::YELLOW,
            );
            return response;
        }

        if max_rtt_ms <= 0.0 {
            eprintln!("ERROR: Invalid RTT range - max_rtt_ms <= 0");
            return response;
        }

        // 2. Set up coordinate transformation using RELATIVE time (0 to duration_ms)
        // instead of absolute timestamps to avoid precision issues
        let data_rect = Rect::from_x_y_ranges(
            0.0..=view_duration_millis as f32,
            max_rtt_ms..=0.0, // Inverted Y-axis: max RTT at top, 0 at bottom (proper RTT visualization)
        );

        if should_debug {
            eprintln!("=== DATA RECT DEBUG ===");
            eprintln!("Data rect X range: 0 to {}", view_duration_millis);
            eprintln!(
                "Data rect Y range: {} to 0 (inverted for proper RTT display)",
                max_rtt_ms
            );
            eprintln!("Screen rect: {:?}", rect);
        }

        let to_screen = RectTransform::from_to(data_rect, rect);

        // 3. Decide on rendering method based on data density
        // Calculate how many points are visible in the current view
        let visible_points_count = self
            .points
            .iter()
            .filter(|p| p.time >= view_start_time && p.time <= view_end_time)
            .count();

        // Use point rendering if we have reasonable density for individual points
        let available_width = rect.width();
        // Much more reasonable threshold: switch to points when we have < 1 point per pixel
        // This will show individual points much more often, giving better visual detail
        let use_point_rendering = (visible_points_count as f32) < available_width;

        if should_debug {
            // DEBUG: Rendering decision
            eprintln!("=== RENDERING DECISION ===");
            eprintln!(
                "View time range: {} to {}",
                view_start_time.format("%H:%M:%S%.3f"),
                view_end_time.format("%H:%M:%S%.3f")
            );
            eprintln!("Visible points in view: {}", visible_points_count);
            eprintln!("Available width: {} pixels", available_width);
            eprintln!(
                "Points per pixel: {:.2}",
                visible_points_count as f32 / available_width
            );
            eprintln!(
                "Threshold for point rendering: < {:.0} points",
                available_width
            );
            eprintln!("Use point rendering: {}", use_point_rendering);
            eprintln!("========================");
        }

        if use_point_rendering {
            self.draw_points(&painter, to_screen, view_start_time, view_end_time);
        } else {
            self.draw_min_max_bars(
                &painter,
                to_screen,
                rect,
                view_start_time,
                view_duration_millis,
                max_rtt_ms,
            );
        }

        // 4. Draw axis labels and grid for better readability
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
    /// Calculate the time range and maximum RTT for the dataset
    fn get_data_ranges(&self) -> (Duration, f32) {
        let first_time = self.points.first().map(|p| p.time).unwrap_or_default();
        let last_time = self.points.last().map(|p| p.time).unwrap_or_default();
        let full_time_range = last_time - first_time;

        // Find the maximum RTT value (preserving sub-millisecond precision) for scaling the Y-axis
        let max_rtt_ms = self
            .points
            .iter()
            .filter_map(|p| p.rtt) // Filter out lost packets (None values)
            .map(|rtt| rtt.num_nanoseconds().unwrap_or(0) as f64 / 1_000_000.0) // Convert to milliseconds with nanosecond precision
            .fold(0.0f64, f64::max) // Find maximum
            .min(MAX_RTT_CLAMP_MS as f64) as f32; // Clamp for better visualization

        (full_time_range, max_rtt_ms)
    }

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
            // Calculate relative time position (0-based from view start) in milliseconds
            let relative_time_ms = (p.time - view_start).num_milliseconds() as f32;

            if let Some(rtt) = p.rtt {
                // Draw successful ping as a blue circle
                // Preserve nanosecond precision in RTT calculations
                let rtt_ms = rtt.num_nanoseconds().unwrap_or(0) as f64 / 1_000_000.0;
                let screen_pos = to_screen * Pos2::new(relative_time_ms, rtt_ms as f32);
                painter.circle_filled(screen_pos, 2.0, success_color);
            } else {
                // Draw lost packet as a red marker at the bottom
                let screen_pos = to_screen * Pos2::new(relative_time_ms, 0.0);
                painter.rect_filled(
                    Rect::from_center_size(screen_pos, egui::Vec2::new(2.0, 8.0)),
                    0.0,
                    lost_color,
                );
            }
        }
    }

    fn draw_min_max_bars(
        &self,
        painter: &Painter,
        to_screen: RectTransform,
        rect: Rect,
        view_start_time: DateTime<Utc>,
        _view_duration_millis: f64,
        _max_rtt_ms: f32,
    ) {
        let bar_color = egui::Color32::from_rgb(100, 200, 255);
        let stroke = Stroke::new(1.0, bar_color);
        let from_screen = to_screen.inverse();

        // Render at sub-pixel resolution for better detail
        let step_size = 0.5; // Half-pixel steps for better resolution
        let mut screen_x = rect.min.x;

        while screen_x <= rect.max.x {
            // Calculate relative time range for this pixel slice (0-based from view start)
            let t_start_relative = from_screen.transform_pos(Pos2::new(screen_x, 0.0)).x as f64;
            let t_end_relative = from_screen
                .transform_pos(Pos2::new(screen_x + step_size, 0.0))
                .x as f64;

            // Convert relative time back to absolute timestamps for data lookup
            let t_start_millis = view_start_time.timestamp_millis() as f64 + t_start_relative;
            let t_end_millis = view_start_time.timestamp_millis() as f64 + t_end_relative;

            // Optimized: Find the first and last data points in this time range
            // Use binary search to find the range efficiently
            let start_idx = self
                .points
                .partition_point(|p| (p.time.timestamp_millis() as f64) < t_start_millis);
            let end_idx = self
                .points
                .partition_point(|p| (p.time.timestamp_millis() as f64) < t_end_millis);

            if start_idx < end_idx {
                let mut point_slice = &self.points[start_idx..end_idx];

                // If we only have 1 point in this pixel, expand to neighboring points to create meaningful bars
                if point_slice.len() == 1 && self.points.len() > 1 {
                    let expand_start = start_idx.saturating_sub(1);
                    let expand_end = (end_idx + 1).min(self.points.len());
                    point_slice = &self.points[expand_start..expand_end];
                }

                let mut slice_min_rtt_ms = f64::INFINITY;
                let mut slice_max_rtt_ms = 0.0f64;
                let mut success_count = 0;
                let mut lost_count = 0;

                for p in point_slice {
                    if let Some(rtt) = p.rtt {
                        // Preserve nanosecond precision in RTT calculations
                        let rtt_ms = rtt.num_nanoseconds().unwrap_or(0) as f64 / 1_000_000.0;
                        slice_min_rtt_ms = slice_min_rtt_ms.min(rtt_ms);
                        slice_max_rtt_ms = slice_max_rtt_ms.max(rtt_ms);
                        success_count += 1;
                    } else {
                        lost_count += 1;
                    }
                }

                // Draw RTT range bar for successful pings
                if success_count > 0 {
                    // Use RTT values directly - RectTransform handles the coordinate mapping
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

                // Draw lost packet indicator
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

    /// Draw axis labels and grid lines for better readability
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
            // With inverted Y-axis range (max_rtt_ms..=0.0), higher RTT values appear at top as expected
            let y_pos = to_screen.transform_pos(Pos2::new(0.0, rtt_ms)).y;

            if y_pos >= rect.min.y && y_pos <= rect.max.y {
                // Draw grid line
                painter.line_segment(
                    [Pos2::new(rect.min.x, y_pos), Pos2::new(rect.max.x, y_pos)],
                    Stroke::new(1.0, grid_color),
                );

                // Draw label
                let label = if rtt_ms >= 1000.0 {
                    format!("{:.1}s", rtt_ms / 1000.0)
                } else {
                    format!("{:.1}ms", rtt_ms)
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
        let num_x_ticks = 6; // More ticks for better time resolution

        for i in 0..=num_x_ticks {
            let time_progress = i as f32 / num_x_ticks as f32;
            let time_millis =
                view_start_time.timestamp_millis() as f32 + (view_duration_millis * time_progress);
            let x_pos = to_screen.transform_pos(Pos2::new(time_millis, 0.0)).x;

            if x_pos >= rect.min.x && x_pos <= rect.max.x {
                // Draw vertical grid line
                painter.line_segment(
                    [Pos2::new(x_pos, rect.min.y), Pos2::new(x_pos, rect.max.y)],
                    Stroke::new(1.0, grid_color),
                );

                // Calculate the actual time for this position
                let actual_time = view_start_time
                    + Duration::milliseconds((view_duration_millis * time_progress) as i64);

                // Format time based on duration span
                let time_label = if view_duration_millis > 3_600_000.0 {
                    // More than 1 hour span: show hours:minutes
                    actual_time.format("%H:%M").to_string()
                } else if view_duration_millis > 60_000.0 {
                    // More than 1 minute span: show minutes:seconds
                    actual_time.format("%M:%S").to_string()
                } else {
                    // Less than 1 minute: show seconds.milliseconds
                    let secs = actual_time.format("%S").to_string();
                    let millis = actual_time.timestamp_millis() % 1000;
                    format!("{}.{:03}s", secs, millis)
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
