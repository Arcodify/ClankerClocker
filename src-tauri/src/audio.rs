use std::io::Cursor;

use parking_lot::Mutex;
use rodio::{DeviceSinkBuilder, Decoder, MixerDeviceSink};

/// Plays notification sounds directly through the OS audio device (ALSA/CoreAudio/WASAPI
/// via rodio+cpal), bypassing the webview entirely. This avoids depending on WebKitGTK's
/// GStreamer-backed <audio> playback, which the AppImage bundle doesn't ship working
/// GStreamer plugins for, and sidesteps browser autoplay-gesture policies on every OS.
pub struct AudioPlayer {
    sink: Mutex<Option<MixerDeviceSink>>,
}

impl AudioPlayer {
    pub fn new() -> Self {
        // Deliberately not `DeviceSinkBuilder::open_default_sink()`: on failure it falls
        // back to scanning every enumerated ALSA device, including raw hardware PCMs
        // (hw:CARD=...) that bypass PipeWire/PulseAudio's shared routing. Grabbing one of
        // those and holding it open for the app's lifetime locks the sound card exclusively,
        // breaking audio for every other app on the system. Only ever try the "default"
        // (server-routed) device; if that fails, disable notification sounds instead of
        // seizing hardware out from under the rest of the system.
        let sink = match DeviceSinkBuilder::from_default_device().and_then(|b| b.open_stream()) {
            Ok(sink) => Some(sink),
            Err(err) => {
                log::warn!("notification audio disabled: could not open output device: {err}");
                None
            }
        };
        Self {
            sink: Mutex::new(sink),
        }
    }

    pub fn play(&self, kind: &str) {
        let sink = self.sink.lock();
        let Some(sink) = sink.as_ref() else {
            return;
        };
        match Decoder::try_from(Cursor::new(sound_bytes(kind))) {
            Ok(source) => sink.mixer().add(source),
            Err(err) => log::warn!("failed to decode notification sound {kind:?}: {err}"),
        }
    }
}

fn sound_bytes(kind: &str) -> &'static [u8] {
    match kind {
        "clock_in_reminder" => include_bytes!("../assets/audio/clock_in_reminder.mp3"),
        "idle_clockout_warning" => include_bytes!("../assets/audio/idle_clockout_warning.mp3"),
        "idle_clockout" => include_bytes!("../assets/audio/idle_clockout.mp3"),
        "scheduled_clockout_warning" => {
            include_bytes!("../assets/audio/scheduled_clockout_warning.mp3")
        }
        "scheduled_clockout" => include_bytes!("../assets/audio/scheduled_clockout.mp3"),
        _ => include_bytes!("../assets/audio/info.mp3"),
    }
}
