use super::{Compress, CompressTo, Error};

#[derive(Debug)]
pub struct LogQuantizer {
    pub data: Vec<u64>,
    pub precision: f32,  // Ratio of maximum log deviation (0.01 => 1%)
    pub zero_point: f32, // Minimum value allowed (autodetected)
    pub max_value: u64,  // Maximum value encoded (for bit calculation)
    pub bits: u8,        // Number of bits required to serialize one value
}
/*
WARN: This library has a problem. It's neither capable to encode zero values or
negative values.
To allow for zero+negative we need:
  - min_significant_value : f32 , which value is actually encoded as non-zero
  - zero_value: Option<u64>, where zero is encoded. If it is at all.

Still, encoding positive or negative-only values would be problematic.

WARN: This library also does not encode NaN, Infinity, Sub-normal or any non-real number.
*/

impl LogQuantizer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn decompress_data(&self, srcdata: &[u64]) -> Result<Vec<f32>, Error> {
        let log_shift: f32 = self.precision.ln_1p();
        let lg_zero_point = self.zero_point.ln();
        let data: Vec<_> = srcdata
            .iter()
            .map(|x| *x as f32 * log_shift)
            .map(|x| x + lg_zero_point)
            .map(|x| x.exp())
            .collect();

        Ok(data)
    }
}

impl Default for LogQuantizer {
    fn default() -> Self {
        Self {
            data: vec![],
            precision: 0.002, // 0.001 => 0.1%
            zero_point: 0.0,
            max_value: 0,
            bits: 0,
        }
    }
}
impl Compress<f32> for LogQuantizer {
    fn setup(
        &mut self,
        _params: std::collections::HashMap<String, crate::dynrmp::variant::Variant>,
    ) -> Result<(), Error> {
        Err(Error::ToDo)
    }

    fn compress(&mut self, data: &[f32]) -> Result<(), Error> {
        let log_shift: f32 = self.precision.ln_1p();
        self.zero_point = data.iter().fold(f32::MAX, |a, b| -> f32 { a.min(*b) });
        let lg_zero_point = self.zero_point.ln();
        self.data = data
            .iter()
            .map(|x| x.ln() - lg_zero_point)
            .map(|x| x / log_shift)
            .map(|x| x.round() as u64)
            .collect();
        self.max_value = self.data.iter().max().copied().unwrap();
        let bits = (self.max_value as f32).log2();
        self.bits = bits.ceil() as u8;
        // dbg!(log_shift);
        dbg!(lg_zero_point);
        dbg!(self.max_value);
        dbg!(bits);
        // dbg!(self.bits);
        Ok(())
    }

    fn serialize(&self) -> Result<Vec<u8>, Error> {
        Err(Error::ToDo)
    }

    fn deserialize(&mut self, _payload: &[u8]) -> Result<(), Error> {
        Err(Error::ToDo)
    }

    fn decompress(&self) -> Result<Vec<f32>, Error> {
        self.decompress_data(&self.data)
    }
    fn debug_name(&self) -> String {
        format!("LogQuantizer<p:{}>", self.precision)
    }
}

impl CompressTo<f32, u64> for LogQuantizer {
    fn get_data(&self) -> Result<&[u64], Error> {
        Ok(&self.data)
    }

    fn decompress_from(&self, srcdata: &[u64]) -> Result<Vec<f32>, Error> {
        self.decompress_data(&srcdata)
    }
}

//// -*---

#[derive(Debug)]
pub struct LinearQuantizer {
    pub data: Vec<u64>,
    pub max_value: u64, // Maximum value encoded (for bit calculation)
    pub min_point: f32, // Minimum value allowed (autodetected)
    pub max_point: f32, // Maximum value allowed (autodetected)
    pub bits: u8,       // Number of bits required to serialize one value
}

impl LinearQuantizer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn decompress_data(&self, srcdata: &[u64]) -> Result<Vec<f32>, Error> {
        let wide = self.max_point - self.min_point;
        let maxval = self.max_value as f32;

        Ok(srcdata
            .iter()
            .map(|x| *x as f32 * wide / maxval)
            .map(|x| x + self.min_point)
            .collect())
    }
}

impl Default for LinearQuantizer {
    fn default() -> Self {
        Self {
            data: vec![],
            max_value: 6080,
            min_point: 0.0,
            max_point: 0.0,
            bits: 0,
        }
    }
}
impl Compress<f32> for LinearQuantizer {
    fn setup(
        &mut self,
        _params: std::collections::HashMap<String, crate::dynrmp::variant::Variant>,
    ) -> Result<(), Error> {
        Err(Error::ToDo)
    }

    fn compress(&mut self, data: &[f32]) -> Result<(), Error> {
        self.min_point = data.iter().fold(f32::MAX, |a, b| -> f32 { a.min(*b) });
        self.max_point = data.iter().fold(f32::MIN, |a, b| -> f32 { a.max(*b) });
        let wide = self.max_point - self.min_point;
        let maxval = self.max_value as f32;
        self.data = data
            .iter()
            .map(|x| x - self.min_point)
            .map(|x| x * maxval / wide)
            .map(|x| x.round() as u64)
            .collect();
        self.bits = maxval.log2().ceil() as u8;
        Ok(())
    }

    fn serialize(&self) -> Result<Vec<u8>, Error> {
        Err(Error::ToDo)
    }

    fn deserialize(&mut self, _payload: &[u8]) -> Result<(), Error> {
        Err(Error::ToDo)
    }

    fn decompress(&self) -> Result<Vec<f32>, Error> {
        self.decompress_data(&self.data)
    }
    fn debug_name(&self) -> String {
        format!("LinearQuantizer<v:{}>", self.max_value)
    }
}

impl CompressTo<f32, u64> for LinearQuantizer {
    fn get_data(&self) -> Result<&[u64], Error> {
        Ok(&self.data)
    }

    fn decompress_from(&self, srcdata: &[u64]) -> Result<Vec<f32>, Error> {
        self.decompress_data(&srcdata)
    }
}
