use crate::data::DataPoint;
use chrono::{DateTime, Duration, Utc};
use egui::{emath::RectTransform, Painter, Pos2, Rect, Response, Sense, Stroke, Ui, Widget};

const MAX_RTT_CLAMP: u16 = 5000; // Clamp max RTT to 5ms for visualization
const POINT_DRAW_DURATION_THRESHOLD: Duration = Duration::minutes(1);

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
        let (full_time_range, max_rtt) = self.get_data_ranges();
        let view_duration = full_time_range / self.zoom as i32;
        let view_start_time = self.points[0].time + Duration::microseconds(self.pan_micros);
        let view_end_time = view_start_time + view_duration;

        // 2. Set up coordinate transformation
        let data_rect = Rect::from_x_y_ranges(
            (view_start_time.timestamp_micros() as f32)..=(view_end_time.timestamp_micros() as f32),
            (max_rtt as f32)..=0.0, // Inverted Y-axis: 0 is at the bottom
        );
        let to_screen = RectTransform::from_to(data_rect, rect);

        // 3. Render
        if view_duration < POINT_DRAW_DURATION_THRESHOLD {
            self.draw_points(&painter, to_screen, view_start_time, view_end_time);
        } else {
            self.draw_min_max_bars(&painter, to_screen, rect, view_start_time);
        }

        response
    }
}

impl<'a> PlotWidget<'a> {
    fn get_data_ranges(&self) -> (Duration, u16) {
        let first_time = self.points.first().map(|p| p.time).unwrap_or_default();
        let last_time = self.points.last().map(|p| p.time).unwrap_or_default();
        let full_time_range = last_time - first_time;

        let max_rtt = self
            .points
            .iter()
            .map(|p| p.rtt_micros)
            .filter(|&rtt| rtt != u16::MAX)
            .max()
            .unwrap_or(0)
            .min(MAX_RTT_CLAMP); // Clamp for better visualization

        (full_time_range, max_rtt)
    }

    fn draw_points(&self, painter: &Painter, to_screen: RectTransform, view_start: DateTime<Utc>, view_end: DateTime<Utc>) {
        let visible_points = self.points.iter().filter(|p| p.time >= view_start && p.time <= view_end);
        let point_color = egui::Color32::from_rgb(100, 200, 255);

        for p in visible_points {
            if p.rtt_micros == u16::MAX { continue; }
            let screen_pos = to_screen * Pos2::new(p.time.timestamp_micros() as f32, p.rtt_micros as f32);
            painter.circle_filled(screen_pos, 2.0, point_color);
        }
    }

    fn draw_min_max_bars(&self, painter: &Painter, to_screen: RectTransform, rect: Rect, view_start: DateTime<Utc>) {
        let bar_color = egui::Color32::from_rgb(100, 200, 255);
        let stroke = Stroke::new(1.0, bar_color);
        let from_screen = to_screen.inverse();

        // Find the starting point for our iteration
        let mut data_idx = self.points.partition_point(|p| p.time < view_start);

        for screen_x in (rect.min.x as i32)..=(rect.max.x as i32) {
            let t_end_micros = from_screen.transform_pos(Pos2::new((screen_x + 1) as f32, 0.0)).x;

            let slice_start_idx = data_idx;
            while data_idx < self.points.len() && (self.points[data_idx].time.timestamp_micros() as f32) < t_end_micros {
                data_idx += 1;
            }

            let point_slice = &self.points[slice_start_idx..data_idx];

            if !point_slice.is_empty() {
                let mut min_rtt = u16::MAX;
                let mut max_rtt = 0;
                let mut points_in_slice = 0;

                for p in point_slice {
                    if p.rtt_micros != u16::MAX {
                        min_rtt = min_rtt.min(p.rtt_micros);
                        max_rtt = max_rtt.max(p.rtt_micros);
                        points_in_slice += 1;
                    }
                }

                if points_in_slice > 0 {
                    let y_min = to_screen.transform_pos(Pos2::new(0.0, min_rtt as f32)).y;
                    let y_max = to_screen.transform_pos(Pos2::new(0.0, max_rtt as f32)).y;
                    painter.line_segment([Pos2::new(screen_x as f32, y_min), Pos2::new(screen_x as f32, y_max)], stroke);
                }
            }
        }
    }
}
