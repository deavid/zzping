# zzpingd

This is part of the zzping suite.

It is done on Rust, single thread. Considered the server of zzping-gui

## Usage

This requires either sudo or setcap:

`sudo setcap cap_net_raw=eip target/debug/zzpingd`

I use `./run.sh` to build + run (debug mode) while developing.

This will load the file `gui_config.ron` with the basic settings.

By default this will ping some hosts at the same time, one of those is
`192.168.0.1`, to change that, see the file `gui_config.ron`.

To select a different file or different folder for this config file, use the
flag:

`-c, --config <config> [default: gui_config.ron]`

Be warned that `./run.sh` does not support flags.

## Config file contents

*   udp_listen_address: IP Address and port where this binary listens to.
*   udp_client_address: IP Address and port where the client GUI is listening.
*   ping_targets: List of Target Host IP to ping.
