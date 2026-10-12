//! Lapacho Voice's bridge: `lapacho-voice` (the desktop's dictation) for Kotlin.

use std::sync::Arc;

use zeroize::Zeroize;

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum VoiceError {
    /// The model folder is incomplete or doesn't load; `reason` is for the user.
    #[error("{reason}")]
    Model { reason: String },
}

/// The files a model folder must hold, so the app can check an import.
#[uniffi::export]
pub fn model_files() -> Vec<String> {
    lapacho_voice::MODEL_FILES.iter().map(|f| f.to_string()).collect()
}

/// Unpacks the model archive the user picked into `dir`. `fd` is the
/// picked file's descriptor (`ParcelFileDescriptor.detachFd()`): Rust owns
/// it from here and closes it.
#[cfg(unix)]
#[uniffi::export]
pub fn import_model(fd: i32, dir: String) -> Result<(), VoiceError> {
    // SAFETY: the descriptor was detached for us and nothing else uses it.
    let file = unsafe { <std::fs::File as std::os::fd::FromRawFd>::from_raw_fd(fd) };
    lapacho_voice::import(std::io::BufReader::new(file), std::path::Path::new(&dir))
        .map_err(|reason| VoiceError::Model { reason })
}

#[derive(uniffi::Object)]
pub struct VoiceModel(lapacho_voice::Dictation);

#[uniffi::export]
impl VoiceModel {
    #[uniffi::constructor]
    pub fn new(dir: String, threads: i32) -> Result<Arc<Self>, VoiceError> {
        lapacho_voice::Dictation::load(std::path::Path::new(&dir), threads)
            .map(|d| Arc::new(Self(d)))
            .map_err(|reason| VoiceError::Model { reason })
    }

    /// The text in `pcm`: mono, 16-bit little-endian, what `AudioRecord`
    /// gives on every phone (float isn't guaranteed). Bytes, not floats:
    /// uniffi would box each float into a Kotlin object. The audio is zeroed
    /// here, both copies.
    pub fn transcribe(&self, sample_rate: i32, mut pcm: Vec<u8>) -> String {
        let mut samples: Vec<f32> =
            pcm.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();
        pcm.zeroize();
        let text = self.0.transcribe(sample_rate, &mut samples);
        samples.zeroize();
        text
    }
}
