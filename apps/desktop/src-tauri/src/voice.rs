//! Hold-to-talk dictation: record while the shortcut is held, transcribe on
//! release, and put the text on the clipboard — the monitor then stores it
//! like anything copied (classified, masked if it is a secret), and Ctrl+V
//! pastes it.
//!
//! The audio lives in memory only and is zeroed once transcribed. The model
//! (`lapacho-voice`) is loaded while you speak and dropped after.

use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use lapacho_voice::Dictation;
use zeroize::Zeroize;

/// Shorter than this is a key bounce, not speech.
const MIN_SECONDS: f32 = 0.3;

/// The recording in progress, if any: dropping or sending on it stops it.
#[derive(Default)]
pub struct Voice(Mutex<Option<Sender<()>>>);

impl Voice {
    pub fn start(&self, model_dir: PathBuf) {
        let mut current = self.0.lock().unwrap();
        if current.is_some() {
            return; // key repeat while held
        }
        let (stop_tx, stop_rx) = mpsc::channel();
        *current = Some(stop_tx);
        std::thread::spawn(move || {
            if let Err(e) = dictate(model_dir, stop_rx) {
                eprintln!("lapacho: dictation: {e}");
            }
        });
    }

    pub fn stop(&self) {
        if let Some(tx) = self.0.lock().unwrap().take() {
            let _ = tx.send(());
        }
    }
}

fn dictate(model_dir: PathBuf, stop: mpsc::Receiver<()>) -> Result<(), String> {
    // ponytail: the model (~1 GB) is loaded on every dictation, while you
    // speak, and dropped after; keep it loaded if the wait shows on short ones.
    let model = std::thread::spawn(move || Dictation::load(&model_dir, 4));

    let (mut samples, rate) = record(stop)?;
    let model = model.join().map_err(|_| "model loader panicked")??;
    let text = if samples.len() as f32 / rate as f32 >= MIN_SECONDS {
        model.transcribe(rate as i32, &samples)
    } else {
        String::new()
    };
    samples.zeroize();
    if text.is_empty() {
        return Ok(());
    }
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(text))
        .map_err(|e| format!("could not set the clipboard: {e}"))
}

/// Records the default microphone, mono, until `stop` fires or is dropped.
fn record(stop: mpsc::Receiver<()>) -> Result<(Vec<f32>, u32), String> {
    let device = cpal::default_host().default_input_device().ok_or("no microphone")?;
    let config = device.default_input_config().map_err(|e| e.to_string())?;
    let channels = config.channels() as usize;
    let rate = config.sample_rate().0;
    let buf = Arc::new(Mutex::new(Vec::<f32>::new()));
    let sink = buf.clone();
    // Downmix to mono by averaging each frame's channels.
    let push = move |frames: &mut dyn Iterator<Item = f32>| {
        let mut out = sink.lock().unwrap();
        let mut acc = 0.0;
        for (i, s) in frames.enumerate() {
            acc += s;
            if i % channels == channels - 1 {
                out.push(acc / channels as f32);
                acc = 0.0;
            }
        }
    };
    let err = |e| eprintln!("lapacho: microphone: {e}");
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => {
            device.build_input_stream(&config.into(), move |d: &[f32], _: &_| push(&mut d.iter().copied()), err, None)
        }
        cpal::SampleFormat::I16 => {
            device.build_input_stream(
                &config.into(),
                move |d: &[i16], _: &_| push(&mut d.iter().map(|&s| s as f32 / 32768.0)),
                err,
                None,
            )
        }
        other => return Err(format!("unsupported microphone format {other}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    let _ = stop.recv();
    drop(stream);
    let mut samples = std::mem::take(&mut *buf.lock().unwrap());
    // Laptop microphones can sit far off zero (this one: +0.21), and the
    // model hears that as noise: centre the recording.
    let mean = samples.iter().sum::<f32>() / samples.len().max(1) as f32;
    samples.iter_mut().for_each(|s| *s -= mean);
    Ok((samples, rate))
}
