//! Local prediction engine (n-gram / dictionary) for Lapacho.
//! Strictly private, offline-only, zero network I/O.

pub struct Predictor;

impl Predictor {
    pub fn new() -> Self {
        Predictor
    }

    pub fn predict(&self, _prefix: &str) -> Vec<String> {
        Vec::new()
    }
}

impl Default for Predictor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_predictor_default() {
        let p = Predictor::default();
        assert!(p.predict("hola").is_empty());
    }
}
