// Copyright 2021 Google LLC
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

// mod basicgraph;
mod custom_errors;
// mod fdq_graph;
// mod firtest;
mod flags;
mod graph_plot;
mod graphutils;
mod gui;
mod udp_comm;

use flags::{Flags, GuiConfig, OtherOpts};
use gui::PingmonGUI;

use anyhow::{Context, Result};
use clap::Parser;

#[derive(Parser)]
#[clap(
    version = "0.2.2-beta2",
    author = "David Martinez Marti <deavidsedice@gmail.com>"
)]
struct Opts {
    #[clap(short, long, default_value = "gui_config.ron")]
    config: String,
    #[clap(short, long)]
    input: Option<String>,
    #[clap(long)]
    firtest: bool,
}

pub fn main() -> Result<()> {
    use env_logger::Env;
    env_logger::Builder::from_env(Env::default().default_filter_or("info"))
        .filter_module("wgpu_core", log::LevelFilter::Error)
        .filter_module("wgpu_hal", log::LevelFilter::Error)
        .init();

    let opts: Opts = Opts::parse();
    let guiconfig = GuiConfig::from_filepath(&opts.config)?;
    let flags = Flags {
        guiconfig,
        otheropts: OtherOpts {
            input_file: opts.input,
        },
    };

    /* TODO: Migrate firtest.rs to iced 0.13
    if opts.firtest {
        iced::application(
            "FirTest",
            firtest::FirTest::update,
            firtest::FirTest::view,
        )
        .subscription(firtest::FirTest::subscription)
        .run_with(|| (firtest::FirTest::new(flags.clone()), iced::Task::none()))
        .context("FirTest error")
    } else */ {
        iced::application(
            "Ping Monitor",
            PingmonGUI::update,
            PingmonGUI::view,
        )
        .subscription(PingmonGUI::subscription)
        .run_with(|| (PingmonGUI::new(flags), iced::Task::none()))
        .context("PingmonGUI errored out")
    }
}
