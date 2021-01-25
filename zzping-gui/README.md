# zzping-gui

This is part of the zzping suite.

It is done on Rust, using Iced for GUI and draws into a Canvas for graph
rendering.

It is considered the client of zzping-daemon, used to visualize the graph of 
latency and packet loss over time.

## Usage

I normally just run `cargo run` on this folder. This will load the file
`gui_config.ron` with the basic settings.

By default this will show the latency of `192.168.0.1` and other hosts that are
preconfigured, to change that, see the file `gui_config.ron`.

To select a different file or different folder for this config file, use the
flag:

`-c, --config <config> [default: gui_config.ron]`

## Config file contents

See `gui_config.ron` file in this folder for additional documentation in 
comments on what parameters are available and what they mean.

## Loading files from disk

It is possible to load files and inspect them. This currently requires a 
conversion of the files stored from zzping-daemon, as it's format is not 
compatible with zzping-gui.

The utilities for transforming zzping-daemon logs are in zzping-lib folder.
Check the `README.md` over there for extensive documentation.

To transform a file, execute:

```bash
$ cd zzpinglib
$ cargo run --release --bin datareadq -- \
  -i ../zzpingd/logs/pingd-log-9.9.9.9-20201201T08.log \
  -o ../dataread-9.9.9.9-20201201T08.log
```

Then execute zzping-gui and add this new file as an argument:

```bash
$ cd zzping-gui/
$ cargo run --release -i ../dataread-9.9.9.9-20201201T08.log
```
