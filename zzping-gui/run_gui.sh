#!/bin/bash

# Set environment variables to work around X11/Wayland issues
# export WINIT_X11_NO_XRANDR=1
# export WINIT_UNIX_BACKEND=x11
# export LIBGL_ALWAYS_SOFTWARE=1

# Alternative: Force Wayland backend if X11 doesn't work
# export WINIT_UNIX_BACKEND=wayland

export ICED_BACKEND=tiny-skia

echo "Running zzping-gui with X11 compatibility settings..."
RUST_BACKTRACE=1 cargo run --release "$@"
