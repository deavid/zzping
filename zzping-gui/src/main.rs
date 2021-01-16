// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

extern crate zzping_lib;

mod custom_errors;
mod fdq_graph;
mod flags;
mod graph_plot;
mod gui;
mod subscr_time;
mod udp_comm;

use flags::{Flags, GuiConfig, OtherOpts};
use gui::PingmonGUI;
use iced::Settings;

use clap::Clap;

#[derive(Clap)]
#[clap(
    version = "0.2.0-beta1",
    author = "David Martinez Marti <deavidsedice@gmail.com>"
)]
struct Opts {
    #[clap(short, long, default_value = "gui_config.ron")]
    config: String,
    #[clap(short, long)]
    input: Option<String>,
}

pub fn main() {
    let opts: Opts = Opts::parse();
    let guiconfig = GuiConfig::from_filepath(&opts.config).unwrap();
    let flags = Flags {
        guiconfig,
        otheropts: OtherOpts {
            input_file: opts.input,
        },
    };
    use iced::Application; // <- Trait run
    PingmonGUI::run(Settings {
        antialiasing: true,
        window: iced::window::Settings {
            size: (1600, 400),
            ..iced::window::Settings::default()
        },
        flags,
        ..Settings::default()
    })
}
