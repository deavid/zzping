//! Handles the quantization of RTT `Duration` values into `u16` symbols.
//!
//! This module is responsible for the lossy conversion of high-precision RTT values
//! into a smaller, more compressible format. The quantization is logarithmic, which
//! provides high precision for small RTTs (where it matters most) and lower
//! precision for very large RTTs.

use std::time::Duration;

/// The symbol used to represent a lost packet.
pub(super) const PACKET_LOST_SYMBOL: u16 = 65535;
/// A reserved symbol used by the compression model to ensure there are always at least two symbols.
pub(super) const DUMMY_SYMBOL: u16 = 65534;
/// The highest symbol value that can represent a valid RTT.
const LAST_SAFE_SYMBOL: u16 = 65530;

/// A logarithmic quantizer for converting `Duration`s to `u16` symbols and back.
///
/// The quantization formula is designed to provide high precision for low RTTs
/// (sub-millisecond) while still being able to represent very large RTTs (up to
/// several seconds) within the `u16` range.
#[derive(Debug, Clone, Copy)]
pub struct Quantizer {
    ln_1_001: f64,
}

impl Quantizer {
    /// Creates a new `Quantizer`.
    pub fn new() -> Self {
        Self {
            ln_1_001: 1.001f64.ln(),
        }
    }

    /// Converts a `Duration` into a `u16` symbol.
    ///
    /// The conversion is lossy. Very large durations will be clamped to the maximum
    /// representable symbol.
    pub fn duration_to_symbol(&self, d: Duration) -> u16 {
        let time_in_ms = d.as_secs_f64() * 1000.0;
        if time_in_ms <= 0.0 {
            return 0;
        }
        let encoded_value = (time_in_ms / 100.0 + 1.0).ln() / self.ln_1_001;
        (encoded_value.round() as u16).clamp(0, LAST_SAFE_SYMBOL)
    }

    /// Converts a `u16` symbol back into a `Duration`.
    ///
    /// This is the reverse of `duration_to_symbol`. The `PACKET_LOST_SYMBOL` is
    /// special-cased to return a `Duration` of `u64::MAX` seconds.
    pub fn symbol_to_duration(&self, symbol: u16) -> Duration {
        if symbol == PACKET_LOST_SYMBOL {
            // Represent packet loss as a very large duration.
            return Duration::from_secs(u64::MAX);
        }
        let time_in_ms = (self.ln_1_001 * symbol as f64).exp() - 1.0;
        let time_in_ms = time_in_ms * 100.0;
        Duration::from_secs_f64(time_in_ms / 1000.0)
    }
}

impl Default for Quantizer {
    fn default() -> Self {
        Self::new()
    }
}
