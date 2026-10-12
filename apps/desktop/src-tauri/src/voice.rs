//! Hold-to-talk dictation: record while the shortcut is held, transcribe on
//! release, and put the text on the clipboard — the monitor then stores it
//! like anything copied (classified, masked if it is a secret), and Ctrl+V
//! pastes it.
//!
//! The audio lives in memory only and is zeroed once transcribed. The model
//! (`lapacho-voice`) is loaded while you speak and dropped after.
//!
//! A small window (`dictation`, never focused, so Ctrl+V still goes where you
//! were typing) shows it happening: a waveform while you speak, then the
//! text that reached the clipboard. It closes itself a moment later.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use lapacho_voice::Dictation;
use tauri::{AppHandle, Manager, PhysicalPosition};
use zeroize::Zeroize;

/// Shorter than this is a key bounce, not speech.
const MIN_SECONDS: f32 = 0.3;

/// How long the result stays on screen.
const SHOW_RESULT: Duration = Duration::from_millis(2500);

/// Dictations so far: the overlay hides only if no newer one started.
static TURN: AtomicU64 = AtomicU64::new(0);

/// The recording in progress, if any: dropping or sending on it stops it.
#[derive(Default)]
pub struct Voice(Mutex<Option<Sender<()>>>);

impl Voice {
    pub fn start(&self, app: AppHandle, model_dir: PathBuf) {
        let mut current = self.0.lock().unwrap();
        if current.is_some() {
            return; // key repeat while held
        }
        let (stop_tx, stop_rx) = mpsc::channel();
        *current = Some(stop_tx);
        let turn = TURN.fetch_add(1, Ordering::SeqCst) + 1;
        show_overlay(&app);
        std::thread::spawn(move || {
            let (js, wait) = match dictate(&app, model_dir, stop_rx) {
                Ok(text) => (format!("dictation.done({})", serde_json::json!(text)), SHOW_RESULT),
                Err(e) => {
                    eprintln!("lapacho: dictation: {e}");
                    (format!("dictation.fail({})", serde_json::json!(e)), SHOW_RESULT * 2)
                }
            };
            overlay(&app, &js);
            std::thread::sleep(wait);
            if TURN.load(Ordering::SeqCst) == turn {
                if let Some(w) = app.get_webview_window("dictation") {
                    let _ = w.hide();
                }
            }
        });
    }

    pub fn stop(&self) {
        if let Some(tx) = self.0.lock().unwrap().take() {
            let _ = tx.send(());
        }
    }
}

/// Bottom centre of the primary screen, above where a taskbar sits.
/// ponytail: Wayland lets no app place its windows; there it goes where the
/// compositor puts it.
fn show_overlay(app: &AppHandle) {
    let Some(w) = app.get_webview_window("dictation") else { return };
    if let (Ok(Some(m)), Ok(size)) = (w.primary_monitor(), w.outer_size()) {
        let (area, pos) = (m.size(), m.position());
        let x = pos.x + (area.width as i32 - size.width as i32) / 2;
        let y = pos.y + area.height as i32 - size.height as i32 - (area.height as i32 / 8);
        let _ = w.set_position(PhysicalPosition::new(x, y));
    }
    let _ = w.show();
    let _ = w.eval("dictation.listen()");
}

fn overlay(app: &AppHandle, js: &str) {
    if let Some(w) = app.get_webview_window("dictation") {
        let _ = w.eval(js);
    }
}

/// The text dictated, now on the clipboard; empty when nothing was understood.
fn dictate(app: &AppHandle, model_dir: PathBuf, stop: mpsc::Receiver<()>) -> Result<String, String> {
    // ponytail: the model (~1 GB) is loaded on every dictation, while you
    // speak, and dropped after; keep it loaded if the wait shows on short ones.
    let model = std::thread::spawn(move || Dictation::load(&model_dir, 4));

    let (mut samples, rate) = record(app, stop)?;
    overlay(app, "dictation.recognizing()");
    let model = model.join().map_err(|_| "model loader panicked")??;
    let text = if samples.len() as f32 / rate as f32 >= MIN_SECONDS {
        model.transcribe(rate as i32, &mut samples)
    } else {
        String::new()
    };
    samples.zeroize();
    if text.is_empty() {
        return Ok(text);
    }
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(text.clone()))
        .map_err(|e| format!("could not set the clipboard: {e}"))?;
    Ok(text)
}

/// Records the default microphone, mono, until `stop` fires or is dropped.
fn record(app: &AppHandle, stop: mpsc::Receiver<()>) -> Result<(Vec<f32>, u32), String> {
    let device = cpal::default_host().default_input_device().ok_or("no microphone")?;
    let config = device.default_input_config().map_err(|e| e.to_string())?;
    let channels = config.channels() as usize;
    let rate = config.sample_rate().0;
    let buf = Arc::new(Mutex::new(Vec::<f32>::new()));
    let sink = buf.clone();
    let app = app.clone();
    // The waveform: every 50 ms, half the swing between the lowest and the
    // highest sample — a microphone's offset (see lapacho-voice) doesn't count.
    let (mut last, mut lo, mut hi) = (Instant::now(), f32::MAX, f32::MIN);
    // Downmix to mono by averaging each frame's channels.
    let mut push = move |frames: &mut dyn Iterator<Item = f32>| {
        let mut out = sink.lock().unwrap();
        let mut acc = 0.0;
        for (i, s) in frames.enumerate() {
            acc += s;
            if i % channels == channels - 1 {
                let m = acc / channels as f32;
                out.push(m);
                (lo, hi) = (lo.min(m), hi.max(m));
                acc = 0.0;
            }
        }
        if last.elapsed() >= Duration::from_millis(50) && hi >= lo {
            overlay(&app, &format!("dictation.level({})", (hi - lo) / 2.0));
            (last, lo, hi) = (Instant::now(), f32::MAX, f32::MIN);
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
    let samples = std::mem::take(&mut *buf.lock().unwrap());
    Ok((samples, rate))
}
