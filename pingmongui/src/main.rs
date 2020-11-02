mod custom_errors;
mod graph_plot;
mod gui;
mod subscr_time;
mod udp_comm;

use gui::PingmonGUI;
use iced::Settings;

pub fn main() {
    use iced::Application; // <- Trait run
    PingmonGUI::run(Settings {
        antialiasing: true,
        window: iced::window::Settings {
            size: (1600, 400),
            ..iced::window::Settings::default()
        },
        ..Settings::default()
    })
}
