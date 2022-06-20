lazy_static! {
    pub static ref GAUSS_WINDOW: CachedWindow = CachedWindow::new_gaussian();
    pub static ref SINC_WINDOW: CachedWindow = CachedWindow::new_sinc();
}
pub struct CachedWindow {
    data: [f64; Self::LENGTH],
}

impl CachedWindow {
    const SIGMAS: usize = 32;
    const RESOLUTION: usize = 1024;
    const LENGTH: usize = Self::SIGMAS * Self::RESOLUTION;
    const MIDPOINT: usize = Self::LENGTH / 2;
    pub fn new_gaussian() -> Self {
        use probability::distribution::Distribution;
        let window = probability::distribution::Gaussian::new(0.0, 1.0);
        let mut data = [0_f64; Self::LENGTH];
        for (n, v) in data.iter_mut().enumerate() {
            let x = (n as f64 - Self::MIDPOINT as f64) / Self::RESOLUTION as f64;
            *v = window.distribution(x);
        }
        Self { data }
    }
    pub fn new_sinc() -> Self {
        use crate::sin_integral::sinc_int_norm;
        let mut data = [0_f64; Self::LENGTH];
        for (n, v) in data.iter_mut().enumerate() {
            let x = (n as f64 - Self::MIDPOINT as f64) / Self::RESOLUTION as f64;
            *v = sinc_int_norm(x);
        }
        Self { data }
    }
    pub fn get(&self, x: f64, width: f64) -> f64 {
        let x = x / width;
        let n = ((x * Self::RESOLUTION as f64) + Self::MIDPOINT as f64 + 0.5) as usize;
        let n = n.clamp(0, Self::LENGTH - 1);
        self.data[n]
    }
}
