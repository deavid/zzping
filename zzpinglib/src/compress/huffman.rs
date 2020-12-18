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
    https://docs.rs/huffman-compress/0.6.0/huffman_compress/


*/

extern crate bit_vec;
extern crate huffman_compress;

use bit_vec::BitVec;
use huffman_compress::CodeBuilder;
use std::collections::HashMap;
use std::iter::FromIterator;

use super::{quantize, Compress, Error};

#[derive(Debug)]
pub struct Huffman {
    // TODO: We should be able to use any source that compress/decompress into u64
    quantizer: quantize::LogQuantizer,
    weights: Vec<(u64, u64)>,
    data: BitVec,
    data_len: usize,
}

impl Default for Huffman {
    fn default() -> Self {
        Self {
            quantizer: quantize::LogQuantizer::default(),
            weights: vec![],
            data: BitVec::new(),
            data_len: 0,
        }
    }
}

impl Compress<f32> for Huffman {
    fn setup(
        &mut self,
        _params: std::collections::HashMap<String, crate::dynrmp::variant::Variant>,
    ) -> Result<(), Error> {
        Err(Error::ToDo)
    }

    fn compress(&mut self, data: &[f32]) -> Result<(), Error> {
        self.quantizer.compress(data)?;
        let mut weights: HashMap<u64, u64> = HashMap::new();
        for k in self.quantizer.data.iter() {
            *weights.entry(*k).or_insert(0) += 1;
        }
        self.weights = weights.iter().map(|(k, v)| (*k, *v)).collect();
        self.weights.sort_unstable_by_key(|(k, _v)| *k);

        let (book, _tree) = CodeBuilder::from_iter(self.weights.iter().copied()).finish();
        let mut total_bits = 0;
        for (k, v) in book.iter() {
            total_bits += v.len() as u64 * weights[k];
        }
        dbg!(total_bits as f32 / data.len() as f32);
        self.data = BitVec::with_capacity(total_bits as usize);
        self.data_len = self.quantizer.data.len();
        for symbol in self.quantizer.data.iter() {
            book.encode(&mut self.data, symbol)
                .map_err(Error::HuffmanEncodeError)?
        }
        Ok(())
    }

    fn serialize(&self) -> Result<Vec<u8>, Error> {
        Err(Error::ToDo)
    }

    fn deserialize(&mut self, _payload: &[u8]) -> Result<(), Error> {
        Err(Error::ToDo)
    }

    fn decompress(&self) -> Result<Vec<f32>, Error> {
        let (_book, tree) = CodeBuilder::from_iter(self.weights.iter().copied()).finish();
        let decoded: Vec<u64> = tree.decoder(&self.data, self.data_len).collect();
        self.quantizer.decompress_data(&decoded)
    }
    fn debug_name(&self) -> String {
        "Huffman<>".to_string()
    }
}
