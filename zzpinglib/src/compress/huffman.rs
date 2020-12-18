/*
    Parameters:
    -----------------
    Input should be f32, not u32.
    Precision: 0.1% -> 1.01 -> ln(1.01)
    Min value: 100us -> ln(100)
    Max value: 30s   -> ln(30_000_000)
    Possible values: (Max - Min) / Precision =
        = (ln(30_000_000) - ln(100)) / ln(1.01) = 1267,4491

    Huffman symbol table encoding:
    --------------------------------
    Full symbol table + frequency: u16,u16
    Frequency only, inc. unused symbols: u16
    Frequency-encoding: i16, negative values do skip.
    Optional, frequency scaling to u8.

    Function based, i.e. tan(1/(x+10))

    Optional extra precision:
    -----------------------------
    Encode error as an extra i8 or i4.

    Other:
    -------------
    Quantization is common in all compression libraries. Common utilities might
    be useful.

*/
use super::{Compress, Error};

#[derive(Debug, Default)]
pub struct Huffman {}

impl Compress<f32> for Huffman {
    fn setup(
        &mut self,
        _params: std::collections::HashMap<String, crate::dynrmp::variant::Variant>,
    ) -> Result<(), Error> {
        Err(Error::ToDo)
    }

    fn compress(&mut self, _data: &[f32]) -> Result<(), Error> {
        Err(Error::ToDo)
    }

    fn serialize(&self) -> Result<Vec<u8>, Error> {
        Err(Error::ToDo)
    }

    fn deserialize(&mut self, _payload: &[u8]) -> Result<(), Error> {
        Err(Error::ToDo)
    }

    fn decompress(&self) -> Result<Vec<f32>, Error> {
        Err(Error::ToDo)
    }
    fn debug_name(&self) -> String {
        "Huffman<>".to_string()
    }
}
