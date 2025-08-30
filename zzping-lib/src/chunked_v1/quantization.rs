use std::time::Duration;

pub(super) const PACKET_LOST_SYMBOL: u16 = 65535;
pub(super) const DUMMY_SYMBOL: u16 = 65534;
const LAST_SAFE_SYMBOL: u16 = 65530;

// TODO: Add quantization accuracy tests to verify this stays within 0.1% or 0.1ms tolerance
// TODO: Test edge cases: zero RTT, maximum valid RTT, boundary values
// TODO: Verify symbol_to_duration(duration_to_symbol(x)) roundtrip accuracy
pub struct Quantizer {
    ln_1_001: f64,
}

impl Quantizer {
    pub fn new() -> Self {
        Self {
            ln_1_001: 1.001f64.ln(),
        }
    }

    pub fn duration_to_symbol(&self, d: Duration) -> u16 {
        let time_in_ms = d.as_secs_f64() * 1000.0;
        if time_in_ms <= 0.0 {
            return 0;
        }
        let encoded_value = (time_in_ms / 100.0 + 1.0).ln() / self.ln_1_001;
        (encoded_value.round() as u16).clamp(0, LAST_SAFE_SYMBOL)
    }

    pub fn symbol_to_duration(&self, symbol: u16) -> Duration {
        if symbol == PACKET_LOST_SYMBOL {
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
