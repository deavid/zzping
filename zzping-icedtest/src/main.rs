use iced::mouse;
use iced::widget::Column;
use iced::widget::canvas;
use iced::widget::canvas::Geometry;
use iced::widget::canvas::Path;
use iced::{Color, Element, Fill, Rectangle, Renderer, Theme};

pub fn main() -> iced::Result {
    iced::application("Two Circles", Circles::update, Circles::view)
        .theme(Circles::theme)
        .run()
}

struct Circles {
    circle1: Circle,
    circle2: Circle,
}

impl Default for Circles {
    fn default() -> Self {
        Self {
            circle1: Circle {
                color: Color::from_rgb8(255, 0, 0), // Red
                cache: canvas::Cache::default(),
            },
            circle2: Circle {
                color: Color::from_rgb8(0, 0, 255), // Blue
                cache: canvas::Cache::default(),
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Message {}

impl Circles {
    fn update(&mut self, _message: Message) {}

    fn view(&self) -> Element<'_, Message> {
        let canvas1 = canvas::Canvas::new(&self.circle1).width(Fill).height(Fill);

        let canvas2 = canvas::Canvas::new(&self.circle2).width(Fill).height(Fill);

        Column::new()
            .push(canvas1)
            .push(canvas2)
            .spacing(20)
            .padding(20)
            .into()
    }

    fn theme(&self) -> Theme {
        Theme::default()
    }
}

impl Default for Circle {
    fn default() -> Self {
        Self {
            color: Color::from_rgb8(255, 0, 0), // Red
            cache: canvas::Cache::default(),
        }
    }
}

struct Circle {
    color: Color,
    cache: canvas::Cache,
}

impl<Message> canvas::Program<Message> for Circle {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let geometry = self.cache.draw(renderer, bounds.size(), |frame| {
            let center = frame.center();
            let radius = frame.width().min(frame.height()) / 4.0;

            let circle = Path::circle(center, radius);
            frame.fill(&circle, self.color);
        });

        vec![geometry]
    }
}
