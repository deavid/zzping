use std::collections::HashMap;

use crate::dynrmp::variant::Variant;

pub mod fft;
pub mod huffman;
pub mod quantize;

// Compression for Vec of f32.
pub trait Compress<T> {
    fn debug_name(&self) -> String;
    fn setup(&mut self, params: HashMap<String, Variant>) -> Result<(), Error>;
    fn compress(&mut self, data: &[T]) -> Result<(), Error>;
    // Compress might be unable to compress all data, where does the remainder go?
    // - Compression library is responsible of appending uncompressed data at the end.
    // Current form suggests that different vectors may have different sizes.
    // - Just single Vec<f32>. Caller responsible of black-magic stuff
    // What happens if compress is called twice? Overwrites? appends?
    // - ??? caller-dependant maybe.
    fn serialize(&self) -> Result<Vec<u8>, Error>;
    fn deserialize(&mut self, payload: &[u8]) -> Result<(), Error>;
    fn decompress(&self) -> Result<Vec<T>, Error>;
}

pub trait CompressTo<T, U>: Compress<T> {
    fn get_data(&self) -> Result<&[U], Error>;
    fn decompress_from(&self, srcdata: &[U]) -> Result<Vec<T>, Error>;
}

// Other compression targets:
// - Packet loss oriented
// - Recv size oriented

#[derive(Debug)]
pub enum Error {
    ToDo,
    AssertError,
    HuffmanEncodeError(huffman_compress::EncodeError),
}
