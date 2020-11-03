# zzping

*Ping your home network while you sleep.*

## Description

This is a collection of tools to monitor home network latency and packet loss.

It currently contains two programs:

* **zzpingd**: A daemon that can be run as root or with setcap that will ping several hosts of your choice every 16ms (60 icmp/s). Outputs statistics via UDP every 100ms.

* **zzping-gui**: A graphical interface that connects to the daemon and shows a graph with both latency and packet loss, up to 1000 last entries (16s) and updated in real time.

zzping is entirely done in Rust. Not exactly for performance reasons (well, maybe), but because the author thought this was a good exercise to learn the language.

These tools are single threaded on purpose, because they look for the lowest
footprint possible on the system that it runs. The daemon barely uses any CPU
resources. The GUI lacks optimization/caching and uses a bit of CPU drawing the graph.

## Motivation

My home network works fine 99% of the time. But often, I see drops while doing
meetings WFH. Also, when gaming online, I tend to get kicked because it seems I lost connection for ~200ms.

I mostly connect to my router via WiFi and Powerline, and both show strange behavior. So I wanted an opensource tool that it's able to monitor it for me and test from and to different hosts so I can analyze where the problem is.

Rust is also growing quite fast and I really like the language, so I used this opportunity to learn more and code something useful with it.


## Caveats

Currently zzping does not have (yet) the functionality needed to be a proper monitor of the network. For example, it does not store anything on disk or database. If the GUI is not working, that data is simply lost. After 16 seconds, the data is no longer viisble on the graph and it's also lost.

There's also other problems, like lack of checking for errors (Ulike division by zero), so both the daemon and gui might segfault at any point.

I only tested this under Debian GNU/Linux bullseye/sid. Should work on other OS
as well.

### ICMP permissions

zzpingd will try to create a socket for ICMP, and therefore it requires either
root user, setuid, or setcap under most/all *nix systems.

A script called `run.sh` is provided to build + setcap, which will make it easier to run it under your regular user without root permissions; but running setcap requires root, so this script uses the `sudo` command internally. Shouldn't be a problem as this script is small enough to be easy to audit.

Once built, you might want to deploy it as a service. This is probably the best way as it will require the least amount of permissions and it will constantly run in background.

## Licensing

All programs on the zzping suite are licensed under the MIT License.

