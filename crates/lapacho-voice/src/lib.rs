//! Dictation: speech in, text out, on the device. No audio or text leaves it.
//!
//! One model, Parakeet TDT 0.6B v3 int8 (NVIDIA, CC-BY-4.0), run by
//! sherpa-onnx: 25 European languages, the language is detected, no setting.
//! Chosen on `speech-eval`'s numbers (FLEURS, 2026-10-09): 3.6 % WER in
//! Spanish, seven times faster than the next model. No Hebrew; that is
//! Whisper's job, later.
//!
//! The model is the folder sherpa-onnx publishes
//! (`sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8`), imported by the user,
//! never downloaded by Lapacho. Recording is the host's: `cpal` on desktop,
//! `AudioRecord` on Android. This crate only turns samples into text.

use std::path::Path;

use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig};

/// The files a model folder must hold.
pub const MODEL_FILES: [&str; 4] = ["encoder.int8.onnx", "decoder.int8.onnx", "joiner.int8.onnx", "tokens.txt"];

/// A loaded model. Loading takes seconds and ~1 GB of RAM, so load once and
/// keep it; `transcribe` can be called from any thread.
pub struct Dictation {
    recognizer: OfflineRecognizer,
}

impl Dictation {
    /// Loads the model in `dir`. `Err` names what is missing or broken, in
    /// English, short: it is shown to the user.
    pub fn load(dir: &Path, threads: i32) -> Result<Self, String> {
        if let Some(f) = MODEL_FILES.iter().find(|f| !dir.join(f).is_file()) {
            return Err(format!("Voice model incomplete: {f} missing in {}", dir.display()));
        }
        let path = |f: &str| Some(dir.join(f).to_string_lossy().into_owned());
        let mut config = OfflineRecognizerConfig::default();
        config.model_config.transducer = OfflineTransducerModelConfig {
            encoder: path("encoder.int8.onnx"),
            decoder: path("decoder.int8.onnx"),
            joiner: path("joiner.int8.onnx"),
        };
        config.model_config.tokens = path("tokens.txt");
        config.model_config.model_type = Some("nemo_transducer".into());
        config.model_config.num_threads = threads;
        let recognizer = OfflineRecognizer::create(&config).ok_or("Voice model could not be loaded")?;
        Ok(Self { recognizer })
    }

    /// The text spoken in `samples` (mono, -1.0..1.0, any sample rate:
    /// sherpa-onnx resamples). Empty when nothing was recognized.
    pub fn transcribe(&self, sample_rate: i32, samples: &[f32]) -> String {
        let stream = self.recognizer.create_stream();
        stream.accept_waveform(sample_rate, samples);
        self.recognizer.decode(&stream);
        stream.get_result().map(|r| r.text.trim().to_string()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_model_says_which_file() {
        let err = Dictation::load(Path::new("/nonexistent"), 1).err().unwrap();
        assert!(err.contains("encoder.int8.onnx"), "{err}");
    }

    /// Needs the model (641 MB), so it runs only when told where it is:
    /// `LAPACHO_VOICE_MODEL=<folder> cargo test -p lapacho-voice`.
    #[test]
    fn transcribes_the_models_own_spanish_sample() {
        let Some(dir) = std::env::var_os("LAPACHO_VOICE_MODEL") else {
            eprintln!("LAPACHO_VOICE_MODEL unset, skipped");
            return;
        };
        let dir = Path::new(&dir);
        let wave = sherpa_onnx::Wave::read(&dir.join("test_wavs/es.wav").to_string_lossy()).unwrap();
        let text = Dictation::load(dir, 4).unwrap().transcribe(wave.sample_rate(), wave.samples());
        eprintln!("es.wav → {text}");
        assert!(text.split_whitespace().count() > 5, "{text}");
    }
}
