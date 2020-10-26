#!/bin/bash

cargo build && \
    sudo setcap cap_net_raw=eip target/debug/pingmon && \
    RUST_LOG=debug ./target/debug/pingmon
