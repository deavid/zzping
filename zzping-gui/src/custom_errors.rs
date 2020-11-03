#[derive(Debug, Clone)]
pub struct UnexpectedError {
    pub t: String,
}

impl UnexpectedError {
    pub fn new(t: &str) -> Self {
        Self { t: t.to_owned() }
    }
}

impl std::fmt::Display for UnexpectedError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "unexpected: {}", self.t)
    }
}

impl std::error::Error for UnexpectedError {}
