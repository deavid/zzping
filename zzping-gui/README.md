# zzping-gui

This is part of the zzping suite. 

It is done on Rust, using Iced for GUI and draws into a Canvas for graph rendering.

It is considered the client of zzpingd, used to visualize the graph of latency and packet loss over time.

## Usage

I normally just run `cargo run` on this folder. This will load the file `gui_config.ron` with the basic settings.

By default this will show the latency of `192.168.0.1`, to change that, see the file `gui_config.ron`.

To select a different file or different folder for this config file, use the flag:

`-c, --config <config>    [default: gui_config.ron]`

## Config file contents

 * udp_listen_address: IP Address and port where the server zzpingd should send the stats.
 * udp_server_address: IP Address and port where the server zzpingd is listening
 * display_address: Target Host IP of the pings that will be graphed
