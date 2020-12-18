use std::collections::HashMap;

use crate::dynrmp::variant::Variant;

pub mod fft;
pub mod huffman;
pub mod quantize;

// Compression for Vec of f32.
pub trait Compress<T>: Default {
    fn setup(&mut self, params: HashMap<String, Variant>);
    fn compress(&mut self, data: &[T]);
    // Compress might be unable to compress all data, where does the remainder go?
    // - Compression library is responsible of appending uncompressed data at the end.
    // Current form suggests that different vectors may have different sizes.
    // - Just single Vec<f32>. Caller responsible of black-magic stuff
    // What happens if compress is called twice? Overwrites? appends?
    // - ??? caller-dependant maybe.
    fn serialize(&self) -> Vec<u8>;
    fn deserialize(&mut self, payload: &[u8]);
    fn decompress(&self) -> Vec<T>;
}

// Other compression targets:
// - Packet loss oriented
// - Recv size oriented
