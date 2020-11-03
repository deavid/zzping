#!/bin/bash

cargo build && \
    sudo setcap cap_net_raw=eip target/debug/zzpingd && \
    RUST_LOG=debug ./target/debug/zzpingd
