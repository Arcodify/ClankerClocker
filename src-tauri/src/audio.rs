use std::io::Cursor;
use std::time::Duration;

use rodio::{DeviceSinkBuilder, Decoder, Source};

/// Plays notification sounds directly through the OS audio device (ALSA/CoreAudio/WASAPI
/// via rodio+cpal), bypassing the webview entirely. This avoids depending on WebKitGTK's
/// GStreamer-backed <audio> playback, which the AppImage bundle doesn't ship working
/// GStreamer plugins for, and sidesteps browser autoplay-gesture policies on every OS.
pub struct AudioPlayer;

impl AudioPlayer {
    pub fn new() -> Self {
        Self
    }

    pub fn play(&self, kind: &str) {
        let kind = kind.to_string();
        // Open the output device only for the few seconds it takes to play a notification,
        // then drop it. On some Linux setups ALSA's "default" PCM still resolves to a raw
        // hardware device instead of routing through PipeWire/PulseAudio (e.g. when the
        // distro's pipewire-alsa glue isn't installed), which locks the sound card for
        // every other app while held open. Holding it only for the clip's duration turns
        // that failure mode into a brief blip instead of requiring the user to quit the
        // app and restart their audio server. Deliberately not
        // `DeviceSinkBuilder::open_default_sink()`: on failure it falls back to scanning
        // every enumerated ALSA device, including raw hardware PCMs, which we never want
        // to pick automatically.
        std::thread::spawn(move || {
            let sink = match DeviceSinkBuilder::from_default_device().and_then(|b| b.open_stream())
            {
                Ok(sink) => sink,
                Err(err) => {
                    log::warn!("notification audio disabled: could not open output device: {err}");
                    return;
                }
            };
            match Decoder::try_from(Cursor::new(sound_bytes(&kind))) {
                Ok(source) => {
                    let duration = source.total_duration().unwrap_or(Duration::from_secs(5));
                    sink.mixer().add(source);
                    std::thread::sleep(duration + Duration::from_millis(300));
                }
                Err(err) => log::warn!("failed to decode notification sound {kind:?}: {err}"),
            }
        });
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
