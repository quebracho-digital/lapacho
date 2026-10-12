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

use std::io::Read;
use std::path::Path;

use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig};

/// The files a model folder must hold.
pub const MODEL_FILES: [&str; 4] = ["encoder.int8.onnx", "decoder.int8.onnx", "joiner.int8.onnx", "tokens.txt"];

/// Unpacks the model from the archive sherpa-onnx publishes
/// (`sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2`) into `dir`: the
/// [`MODEL_FILES`] only, found by name wherever they sit in it, so nothing
/// else in the archive (test audio, scripts) lands on the device, and no
/// path inside it can point outside `dir`. Each file is written under a
/// temporary name and renamed when complete: an interrupted import leaves
/// no half-written model behind.
pub fn import(archive: impl Read, dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut tar = tar::Archive::new(bzip2::read::BzDecoder::new(archive));
    let mut found = Vec::new();
    for entry in tar.entries().map_err(|_| "Not a voice model archive")? {
        let mut entry = entry.map_err(|_| "The archive is damaged")?;
        let path = entry.path().map_err(|e| e.to_string())?;
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        let Some(&name) = MODEL_FILES.iter().find(|f| **f == name) else { continue };
        let part = dir.join(format!("{name}.part"));
        let mut out = std::fs::File::create(&part).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut out).map_err(|_| "The archive is damaged")?;
        std::fs::rename(&part, dir.join(name)).map_err(|e| e.to_string())?;
        found.push(name);
    }
    match MODEL_FILES.iter().find(|f| !found.contains(f)) {
        Some(f) => Err(format!("Not the voice model: {f} isn't in the archive")),
        None => Ok(()),
    }
}

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
    ///
    /// The recording is centred first: microphones can sit far off zero (a
    /// laptop's measured +0.21), and the model hears that offset as noise —
    /// a sentence it got word for word came back as nonsense.
    pub fn transcribe(&self, sample_rate: i32, samples: &mut [f32]) -> String {
        let mean = samples.iter().sum::<f32>() / samples.len().max(1) as f32;
        samples.iter_mut().for_each(|s| *s -= mean);
        let stream = self.recognizer.create_stream();
        stream.accept_waveform(sample_rate, samples);
        self.recognizer.decode(&stream);
        stream.get_result().map(|r| r.text.trim().to_string()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small archive shaped like sherpa-onnx's: a top folder, the model
    /// files, and things that must not be extracted.
    fn archive(names: &[&str]) -> Vec<u8> {
        let mut tar = tar::Builder::new(Vec::new());
        for name in names {
            let mut h = tar::Header::new_gnu();
            h.set_size(name.len() as u64);
            h.set_cksum();
            tar.append_data(&mut h, format!("parakeet/{name}"), name.as_bytes()).unwrap();
        }
        let mut bz = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
        std::io::Write::write_all(&mut bz, &tar.into_inner().unwrap()).unwrap();
        bz.finish().unwrap()
    }

    #[test]
    fn import_takes_only_the_model_files() {
        let dir = std::env::temp_dir().join(format!("lv_import_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut names = MODEL_FILES.to_vec();
        names.push("test_wavs/es.wav");
        import(&archive(&names)[..], &dir).unwrap();
        let mut got: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        got.sort();
        let mut want = MODEL_FILES.map(String::from).to_vec();
        want.sort();
        assert_eq!(got, want);
        assert_eq!(std::fs::read_to_string(dir.join("tokens.txt")).unwrap(), "tokens.txt");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_refuses_what_is_not_the_model() {
        let dir = std::env::temp_dir().join(format!("lv_refuse_{}", std::process::id()));
        let err = import(&archive(&["tokens.txt"])[..], &dir).unwrap_err();
        assert!(err.contains("encoder.int8.onnx"), "{err}");
        assert!(import(&b"not bzip2"[..], &dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

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
        // The offset a laptop microphone adds: must not change the result.
        let mut samples: Vec<f32> = wave.samples().iter().map(|s| s + 0.21).collect();
        let text = Dictation::load(dir, 4).unwrap().transcribe(wave.sample_rate(), &mut samples);
        eprintln!("es.wav → {text}");
        assert!(text.contains("tu país por ti"), "{text}");
    }
}
