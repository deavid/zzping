# zzping

*Ping your home network while you sleep.*

## Description

This is a collection of tools to monitor home network latency and packet loss.

It currently contains two main programs and other utilities:

*   **zzping-daemon**: A daemon that can be run as root or with setcap that will
    ping several hosts of your choice at the ping rate configured (1-500 icmp/s)
    . Outputs statistics on console, via UDP and logs them to disk several times
    per second (configurable).

*   **zzping-gui**: A graphical interface that connects to the daemon and shows
    a graph with both latency and packet loss, only the last N entries and
    updates in real time. It can also read files from disk.

*   **zzping-lib**: Common tooling for reading and writting messages for all
    binaries. This folder also contains different tools intended to inspect and
    convert between different formats.

zzping is entirely done in Rust. Not exactly for performance reasons (well,
maybe), but because the I thought this was a good exercise to learn the language.

These tools are lightly threaded on purpose, because they look for the lowest
footprint possible on the system that it runs. The daemon barely uses any CPU
resources. The GUI lacks optimization/caching and uses a bit of CPU drawing the
graph.

## Motivation

My home network works fine 99% of the time. But often, I see drops while doing
meetings WFH. Also, when gaming online, I tend to get kicked because it seems I
lost connection for ~200ms.

I mostly connect to my router via WiFi and Powerline, and both show strange
behavior. So I wanted an opensource tool that it's able to monitor it for me and
test from and to different hosts so I can analyze where the problem is.

Rust is also growing quite fast and I really like the language, so I used this
opportunity to learn more and code something useful with it.

## Caveats

Currently zzping does not have (yet) the functionality needed to be a proper
monitor of the network. If the GUI is not working, that data is simply lost. 
After some time, the data is no longer visible on the graph and it's also lost,
unless the logs are converted and loaded in the GUI for later inspection.

There's also other problems, like lack of checking for errors (like division by
zero), so both the daemon and gui might segfault at any point. (Some of them
have been covered by now)

I only tested this under Debian GNU/Linux bullseye/sid, but it should work on
other OS as well.

### ICMP permissions

zzpingd will try to create a socket for ICMP, and therefore it requires either
root user, setuid, or setcap under most/all *nix systems.

A script called `run.sh` is provided to build + setcap, which will make it
easier to run it under your regular user without root permissions; but running
setcap requires root, so this script uses the `sudo` command internally.
Shouldn't be a problem as this script is small enough to be easy to audit.

Once built, you might want to deploy it as a service. This is probably the best
way as it will require the least amount of permissions and it will constantly
run in background.

## Licensing

All programs on the zzping suite are licensed under the Apache 2 License.

## How To

### Packages Required

(for Ubuntu 18.04)

Bootstrapping:

  * git - cloning this repo. (You could instead download a ZIP from github)
  * curl - to download and install rustup

Building zzping-daemon & zzpinglib:

  *  build-essential - some dependencies require GCC and friends

Building zzping-gui:

  * X11 related stuff:
    pkgconf
    libx11-dev
    libxrender-dev
    xserver-xorg-dev
    libexpat1-dev
 * Vulkan: (Graphic engine)
    libvulkan 
    libvulkan-dev 
    mesa-vulkan-drivers 
    vulkan-utils

(mesa-vulkan-drivers is for Intel cards. NVidia has its own package)

NOTE: mesa-vulkan-drivers or equivalent is needed to **run** the program but not
for building it.

### Installing all dependencies

```bash
$ sudo apt install git curl 
$ sudo apt install build-essential
$ sudo apt install pkgconf libx11-dev libxrender-dev xserver-xorg-dev libexpat1-dev
$ sudo apt install libvulkan libvulkan-dev mesa-vulkan-drivers vulkan-utils
```

Install RustUp as recommended by the official docs:

```bash
$ curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

See: https://www.rust-lang.org/tools/install


### Downloading this repository

As any other git repo:

```
$ cd
$ mkdir -p git/rust
$ cd git/rust
$ git clone https://github.com/deavid/zzping.git
$ cd zzping
$ ls
```

If you prefer to use the beta version, also do:

```
$ git checkout beta
```

### Updating this repository

As usual in git:

```
$ git pull
```

If this has conflicts, do a backup of the problematic files and do:

```
$ git reset --hard
$ git pull
```

This will overwrite local changes and pull should work. It doesn't clear any logs
but it will reset any config change you might have made.

### Compiling zzping

This repository contains three programs, just build all three:

```
$ cd zzpinglib; cargo build; cargo build --release; cd ..
$ cd zzpingd; cargo build; cargo build --release; cd ..
$ cd zzping-gui; cargo build; cargo build --release; cd ..
```

### Start the daemon to begin pinging hosts

Configure target hosts in zzpingd/daemon_config.ron and execute:

```
$ cd zzping-daemon/
$ ./run.sh
```

This will ask for sudo password. It is needed to create ICMP packets. run.sh
will give the bare minimum permissions to the binary so it won't run as root.

Resulting logs will be stored in zzping-daemon/logs; a file will be created per
each target host and each clock hour.

WARNING: Logs are overwritten without notice if they have the same name. Restarting
zzping-daemon will overwrite the last log if it's on the same clock hour.

### Launch the GUI to see it in realtime

Configure zzping-gui/gui_config.ron. Change display_address so it contains the 
list of target hosts that you want to see.

Then launch:

```
$ cd zzping-gui/
$ cargo run --release
```

This will connect to the zzping-daemon via UDP. By default is configured to 
localhost using ports 7878+7879; this should work as is unless you need to read
across the network.

NOTE: The UDP protocol lacks authentication and encryption. It also doesn't
support multiple connections. Anyone could connect to the tools if the port is
accessible.

### Launch the GUI to inspect past data

Currently the log files from zzping-daemon are incompatible with zzping-gui
because it uses a newer slimmer format. To see the logs they need to be converted.

The utilities to read and write these formats are inside zzping-lib.

To convert a file execute:

```
$ cd zzping-lib
$ cargo run --release --bin datareadq -- \
  -i ../zzpingd/logs/pingd-log-9.9.9.9-20201201T08.log \
  -o ../dataread-9.9.9.9-20201201T08.log
```

Then we can launch the GUI passing a flag with the file to open:

```
$ cd zzping-gui/
$ cargo run --release -i ../dataread-9.9.9.9-20201201T08.log
```

### Concatenating several files into a single file

The graphical interface only supports one file at a time, but it is possible
to concatenate files. Simply pass a list of files to datareadq and it will 
create a single complete file:

```
$ cargo run --release --bin datareadq -- \
  -i ../zzpingd/logs/pingd-log-9.9.9.9-20201201T*.log \
  -o ../dataread-9.9.9.9-20201201-day.log
```

We need to take into account that the files are appended in the same order that
we pass them into the flag and Bash (or other shells) expand wildcards 
alphabetically. This should work correctly, but if the file gets generated
in a different order it will cause severe visualization problems.

If there are gaps in the data the visualization will have minor problems, but
overall still looks acceptable.

Please don't mix files from different hosts, as these don't contain host data
and zzping-gui will mix them up.

It is possible to open a full day or a week worth of pings, but it's not 
recommended to go further as zzping-gui will keep everything on memory.

If you need to open months/years worth of data, have a look on the fdqread
utility inside zzping-lib, as it can aggregate data. This will reduce the timing
resolution and therefore will be easier on zzping-gui to show.