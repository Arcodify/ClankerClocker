/// Returns true if the microphone is currently being captured by any
/// process — the same low-level signal each OS uses to light up its own
/// mic-in-use indicator (macOS menu-bar dot, Windows 11 taskbar icon, the
/// Linux desktop's mic indicator). Deliberately not matching app names or
/// window titles against a hardcoded list of call apps (Zoom, Meet,
/// Discord, ...): that breaks the moment a title format, locale, or app
/// changes. A call almost always has the mic open, even while muted in-app
/// (apps keep the stream open and silence it locally rather than closing
/// it), so this stays accurate across mute toggles.
pub fn mic_active() -> bool {
    #[cfg(target_os = "linux")]
    return linux_mic_active();

    #[cfg(target_os = "macos")]
    return macos_mic_active();

    #[cfg(target_os = "windows")]
    return windows_mic_active();

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    false
}

// ── Linux: PipeWire/PulseAudio recording streams ────────────────────────────
// `pactl` talks to PipeWire's pulse-compat layer (already required — see the
// pipewire-alsa fix for notification audio) or a real PulseAudio daemon.
// Any active source-output is something actively recording from an input
// device right now.

#[cfg(target_os = "linux")]
fn linux_mic_active() -> bool {
    use std::process::Command;

    Command::new("pactl")
        .args(["list", "short", "source-outputs"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| !o.stdout.iter().all(u8::is_ascii_whitespace))
        .unwrap_or(false)
}

// ── macOS: CoreAudio "is running somewhere" ──────────────────────────────────
// Ask the default input device directly via the same HAL property macOS
// itself polls to drive the orange mic-in-use dot. Linked straight against
// the CoreAudio framework (already pulled in transitively via rodio/cpal for
// notification playback) so this needs no extra crate.

#[cfg(target_os = "macos")]
fn macos_mic_active() -> bool {
    use std::ffi::c_void;

    type OsStatus = i32;
    type AudioObjectId = u32;
    type AudioObjectPropertySelector = u32;
    type AudioObjectPropertyScope = u32;
    type AudioObjectPropertyElement = u32;

    #[repr(C)]
    struct AudioObjectPropertyAddress {
        selector: AudioObjectPropertySelector,
        scope: AudioObjectPropertyScope,
        element: AudioObjectPropertyElement,
    }

    const K_AUDIO_OBJECT_SYSTEM_OBJECT: AudioObjectId = 1;
    const K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL: u32 = 0x676c_6f62; // 'glob'
    const K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN: u32 = 0;
    const K_AUDIO_HARDWARE_PROPERTY_DEFAULT_INPUT_DEVICE: u32 = 0x6449_6e20; // 'dIn '
    const K_AUDIO_DEVICE_PROPERTY_DEVICE_IS_RUNNING_SOMEWHERE: u32 = 0x676f_6e65; // 'gone'
    const K_AUDIO_OBJECT_UNKNOWN: u32 = 0;

    #[link(name = "CoreAudio", kind = "framework")]
    extern "C" {
        fn AudioObjectGetPropertyData(
            in_object_id: AudioObjectId,
            in_address: *const AudioObjectPropertyAddress,
            in_qualifier_data_size: u32,
            in_qualifier_data: *const c_void,
            io_data_size: *mut u32,
            out_data: *mut c_void,
        ) -> OsStatus;
    }

    fn get_u32_property(object_id: AudioObjectId, selector: u32) -> Option<u32> {
        let address = AudioObjectPropertyAddress {
            selector,
            scope: K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL,
            element: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
        };
        let mut value: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        let status = unsafe {
            AudioObjectGetPropertyData(
                object_id,
                &address,
                0,
                std::ptr::null(),
                &mut size,
                &mut value as *mut u32 as *mut c_void,
            )
        };
        (status == 0).then_some(value)
    }

    let Some(device_id) = get_u32_property(
        K_AUDIO_OBJECT_SYSTEM_OBJECT,
        K_AUDIO_HARDWARE_PROPERTY_DEFAULT_INPUT_DEVICE,
    ) else {
        return false;
    };
    if device_id == K_AUDIO_OBJECT_UNKNOWN {
        return false;
    }

    get_u32_property(
        device_id,
        K_AUDIO_DEVICE_PROPERTY_DEVICE_IS_RUNNING_SOMEWHERE,
    )
    .map(|v| v != 0)
    .unwrap_or(false)
}

// ── Windows: WASAPI capture sessions ────────────────────────────────────────
// Ask the audio engine directly: walk every active capture endpoint (mic,
// headset, webcam mic, ...) and check whether any app has a capture session
// in the Active state, i.e. is pulling audio from it right now. This is the
// ground truth the consent store below is only a bookkeeping copy of, and
// it keeps working on machines where Windows stops updating that store —
// seen in the field: a live Discord call left no trace in the registry.

#[cfg(target_os = "windows")]
fn windows_mic_active() -> bool {
    wasapi_capture_active().unwrap_or(false) || consent_store_mic_active()
}

#[cfg(target_os = "windows")]
fn wasapi_capture_active() -> windows::core::Result<bool> {
    use windows::Win32::System::Com::{
        CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED,
    };

    // Runs on a tokio blocking-pool thread, which may or may not already be
    // in a COM apartment. Only undo the init we actually performed.
    let inited = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    // Inner fn so every COM pointer is released before CoUninitialize.
    let result = any_capture_session_active();
    if inited {
        unsafe { CoUninitialize() };
    }
    result
}

#[cfg(target_os = "windows")]
fn any_capture_session_active() -> windows::core::Result<bool> {
    use windows::Win32::Media::Audio::{
        eCapture, AudioSessionStateActive, IAudioSessionManager2, IMMDeviceEnumerator,
        MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};

    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let devices = enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE)?;
        for i in 0..devices.GetCount()? {
            // One flaky device shouldn't hide a call on another.
            let Ok(device) = devices.Item(i) else { continue };
            let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else {
                continue;
            };
            let Ok(sessions) = manager.GetSessionEnumerator() else { continue };
            for j in 0..sessions.GetCount().unwrap_or(0) {
                let Ok(session) = sessions.GetSession(j) else { continue };
                if matches!(session.GetState(), Ok(s) if s == AudioSessionStateActive) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}

// ── Windows fallback: CapabilityAccessManager consent store ──────────────────
// Windows stamps LastUsedTimeStop to 0 for the duration an app holds the
// microphone open and to a real FILETIME once it releases it — the same
// per-user, no-admin-required bookkeeping that feeds Settings > Privacy's
// "recently used" mic list and the taskbar mic-in-use icon. Kept as a
// fallback in case WASAPI enumeration fails outright.

#[cfg(target_os = "windows")]
fn consent_store_mic_active() -> bool {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let out = Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone",
            "/s",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()
        // reg.exe prints in the console's OEM/ANSI codepage, not UTF-8. Any
        // subkey with a non-ASCII byte (an app path like C:#Users#José#...)
        // made strict from_utf8 reject the whole output and silently report
        // "no call". The value names and 0x0 we match are pure ASCII, so
        // lossy decoding keeps them intact.
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();

    out.lines()
        .filter(|l| l.trim_start().starts_with("LastUsedTimeStop"))
        .any(|l| l.trim_end().ends_with("0x0"))
}
