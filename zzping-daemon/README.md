# zzping-daemon

This is part of the zzping suite.

It is done on Rust, two threads (A thread is dedicated to receive all data from
the network, the main thread does everything else). 

Considered the server of zzping-gui

## Usage

This requires either sudo or setcap:

`sudo setcap cap_net_raw=eip target/debug/zzpingd`

I use `./run.sh` to build + run (debug mode) while developing.

This will load the file `daemon_config.ron` with the basic settings.

By default this will ping some hosts at the same time, one of those is
`192.168.0.1`, to change that, see the file `daemon_config.ron`.

To select a different file or different folder for this config file, use the
flag:

`-c, --config <config> [default: gui_config.ron]`

Be warned that `./run.sh` does not support flags.

## Config file contents

See `daemon_config.ron` in this folder for additional documentation in comments.