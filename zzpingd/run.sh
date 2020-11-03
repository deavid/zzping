#!/bin/bash

cargo build && \
    sudo setcap cap_net_raw=eip target/debug/pingmond && \
    RUST_LOG=debug ./target/debug/pingmond
