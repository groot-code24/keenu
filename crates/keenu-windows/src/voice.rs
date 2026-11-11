// SPDX-License-Identifier: GPL-3.0-or-later

//! The microphone and the system's voices. A recording only runs between `start` and `stop`, on a thread
//! that exists for just that long: when no recording is under way the device is not open at all.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use keenu_platform::{AudioDevice, Availability, Recording, Result, VoiceProvider};
use windows::Win32::Foundation::{ERROR_SUCCESS, S_FALSE};
use windows::Win32::Media::Audio::{
    CALLBACK_NULL, HWAVEIN, WAVE_FORMAT_PCM, WAVE_MAPPER, WAVEFORMATEX, WAVEHDR, WAVEINCAPSW,
    WHDR_DONE, waveInAddBuffer, waveInClose, waveInGetDevCapsW, waveInGetNumDevs, waveInOpen,
    waveInPrepareHeader, waveInReset, waveInStart, waveInUnprepareHeader,
};
use windows::Win32::Media::Speech::{ISpVoice, SpVoice};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
use windows_core::{HSTRING, Interface, PCWSTR};

const SAMPLE_RATE: u32 = 16_000;
/// Half a second a buffer.
const BUFFER_SAMPLES: usize = SAMPLE_RATE as usize / 2;
/// The longest recording kept, in case it is never stopped.
const MAX_SECONDS: u64 = 300;
const MMSYSERR_NOERROR: u32 = 0;

#[derive(Default)]
pub struct Voice {
    session: Mutex<Option<Session>>,
    /// Asks what is being said to stop.
    stop_speech: AtomicBool,
}

/// SPEAKFLAGS: return at once, and throw away what was being said.
const SPF_ASYNC: u32 = 1;
const SPF_PURGEBEFORESPEAK: u32 = 2;

struct Session {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<std::result::Result<Vec<i16>, String>>,
}

/// Windows' own privacy switch for desktop apps' use of the microphone.
fn microphone_allowed() -> bool {
    let subkey = HSTRING::from(
        "Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\microphone",
    );
    let name = HSTRING::from("Value");
    let mut buffer = [0u16; 16];
    let mut size = (buffer.len() * 2) as u32;
    // SAFETY: `buffer` and its size are correct for a short string value.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if status != ERROR_SUCCESS {
        // Never set: Windows' default, which allows it.
        return true;
    }
    let length = buffer
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..length]) != "Deny"
}

fn check(result: u32, what: &str) -> std::result::Result<(), String> {
    if result == MMSYSERR_NOERROR {
        Ok(())
    } else {
        Err(format!(
            "{what} failed (error {result}). Is the microphone still connected?"
        ))
    }
}

fn record(device: u32, stop: &AtomicBool) -> std::result::Result<Vec<i16>, String> {
    let format = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_PCM as u16,
        nChannels: 1,
        nSamplesPerSec: SAMPLE_RATE,
        nAvgBytesPerSec: SAMPLE_RATE * 2,
        nBlockAlign: 2,
        wBitsPerSample: 16,
        cbSize: 0,
    };
    let mut handle = HWAVEIN::default();
    // SAFETY: `handle` is closed before leaving; the headers and buffers stay in place until after reset.
    unsafe {
        check(
            waveInOpen(
                Some(&mut handle),
                device,
                &format,
                None,
                None,
                CALLBACK_NULL,
            ),
            "Opening the microphone",
        )?;
        let mut buffers = vec![vec![0i16; BUFFER_SAMPLES]; 4];
        let mut headers: Vec<WAVEHDR> = buffers
            .iter_mut()
            .map(|buffer| WAVEHDR {
                lpData: windows_core::PSTR(buffer.as_mut_ptr().cast()),
                dwBufferLength: (buffer.len() * 2) as u32,
                ..WAVEHDR::default()
            })
            .collect();
        let header_size = size_of::<WAVEHDR>() as u32;
        let mut samples = Vec::new();
        let result = (|| -> std::result::Result<(), String> {
            for header in &mut headers {
                check(
                    waveInPrepareHeader(handle, header, header_size),
                    "Preparing the microphone",
                )?;
                check(
                    waveInAddBuffer(handle, header, header_size),
                    "Reading the microphone",
                )?;
            }
            check(waveInStart(handle), "Starting the microphone")?;
            let started = Instant::now();
            while !stop.load(Ordering::Relaxed) && started.elapsed().as_secs() < MAX_SECONDS {
                let mut idle = true;
                for (index, header) in headers.iter_mut().enumerate() {
                    if header.dwFlags & WHDR_DONE != 0 {
                        idle = false;
                        let got = header.dwBytesRecorded as usize / 2;
                        samples.extend_from_slice(&buffers[index][..got]);
                        header.dwFlags &= !WHDR_DONE;
                        // Fails when the device has been unplugged.
                        check(
                            waveInAddBuffer(handle, header, header_size),
                            "Reading the microphone",
                        )?;
                    }
                }
                if idle {
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
            Ok(())
        })();
        let _ = waveInReset(handle);
        for (index, header) in headers.iter_mut().enumerate() {
            if result.is_ok() && header.dwFlags & WHDR_DONE != 0 {
                let got = header.dwBytesRecorded as usize / 2;
                samples.extend_from_slice(&buffers[index][..got]);
            }
            let _ = waveInUnprepareHeader(handle, header, header_size);
        }
        let _ = waveInClose(handle);
        result?;
        Ok(samples)
    }
}

/// Listening with Windows' built-in engine, which only has a .NET interface. The script is part of this
/// program and takes no input; it prints the words it heard.
const LISTEN_SCRIPT: &str = r"
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Speech
$engine=New-Object System.Speech.Recognition.SpeechRecognitionEngine
try {
  $engine.LoadGrammar((New-Object System.Speech.Recognition.DictationGrammar))
  $engine.SetInputToDefaultAudioDevice()
  $result=$engine.Recognize([TimeSpan]::FromSeconds(12))
  if ($null -ne $result) { [Console]::OutputEncoding=[Text.Encoding]::UTF8; [Console]::Out.Write($result.Text) }
} finally { $engine.Dispose() }
";

/// Listens for one phrase with Windows' built-in engine, on this computer, and prints `heard`. The phrase is letters and
/// spaces only; Windows' engine holds nothing it hears beyond this process.
fn wake_script(phrase: &str) -> String {
    let phrase: String = phrase
        .chars()
        .filter(|c| c.is_alphabetic() || *c == ' ')
        .collect();
    format!(
        r"
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Speech
$engine=New-Object System.Speech.Recognition.SpeechRecognitionEngine
try {{
  $builder=New-Object System.Speech.Recognition.GrammarBuilder('{phrase}')
  $engine.LoadGrammar((New-Object System.Speech.Recognition.Grammar($builder)))
  $engine.SetInputToDefaultAudioDevice()
  while ($true) {{
    $result=$engine.Recognize([TimeSpan]::FromSeconds(5))
    if ($null -ne $result -and $result.Confidence -ge 0.75) {{ [Console]::Out.Write('heard'); break }}
  }}
}} finally {{ $engine.Dispose() }}
"
    )
}

impl VoiceProvider for Voice {
    fn availability(&self) -> Availability {
        if !microphone_allowed() {
            return Availability::NeedsPermission(
                "Windows has the microphone turned off: Settings > Privacy & security > Microphone.".into(),
            );
        }
        // SAFETY: only counts devices.
        if unsafe { waveInGetNumDevs() } == 0 {
            return Availability::Unsupported("No microphone was found.".into());
        }
        Availability::Available
    }

    fn devices(&self) -> Result<Vec<AudioDevice>> {
        // SAFETY: only counts devices.
        let count = unsafe { waveInGetNumDevs() };
        let mut devices = vec![AudioDevice {
            id: String::new(),
            name: "System default".into(),
            is_default: true,
        }];
        for index in 0..count {
            let mut caps = WAVEINCAPSW::default();
            // SAFETY: `caps` is the size given.
            let result = unsafe {
                waveInGetDevCapsW(index as usize, &mut caps, size_of::<WAVEINCAPSW>() as u32)
            };
            if result == MMSYSERR_NOERROR {
                // The structure is packed: copied out, not borrowed.
                let name = caps.szPname;
                let name_end = name
                    .iter()
                    .position(|&unit| unit == 0)
                    .unwrap_or(name.len());
                devices.push(AudioDevice {
                    id: index.to_string(),
                    name: String::from_utf16_lossy(&name[..name_end]),
                    is_default: false,
                });
            }
        }
        Ok(devices)
    }

    fn start(&self, device: &str) -> Result<()> {
        if let Some(why) = self.availability().reason() {
            return Err(why.to_owned());
        }
        let mut session = self
            .session
            .lock()
            .map_err(|_| "The microphone state is damaged.")?;
        if session.is_some() {
            return Err("Already recording.".into());
        }
        let id = if device.is_empty() {
            WAVE_MAPPER
        } else {
            device
                .parse()
                .map_err(|_| "That microphone is no longer there.")?
        };
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::Builder::new()
            .name("keenu-microphone".into())
            .spawn(move || record(id, &flag))
            .map_err(|err| err.to_string())?;
        *session = Some(Session { stop, thread });
        Ok(())
    }

    fn stop(&self) -> Result<Recording> {
        let session = self
            .session
            .lock()
            .map_err(|_| "The microphone state is damaged.")?
            .take();
        let session = session.ok_or("Not recording.")?;
        session.stop.store(true, Ordering::Relaxed);
        let samples = session
            .thread
            .join()
            .map_err(|_| "The recording thread failed.".to_owned())??;
        Ok(Recording {
            sample_rate: SAMPLE_RATE,
            samples,
        })
    }

    fn cancel(&self) -> Result<()> {
        self.stop().map(drop)
    }

    fn recording(&self) -> bool {
        self.session.lock().is_ok_and(|session| {
            session
                .as_ref()
                .is_some_and(|session| !session.thread.is_finished())
        })
    }

    fn listen_local(&self) -> Result<String> {
        if let Some(why) = self.availability().reason() {
            return Err(why.to_owned());
        }
        let encoded: Vec<u8> = LISTEN_SCRIPT
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let output = std::process::Command::new(crate::system::system32(
            "WindowsPowerShell\\v1.0\\powershell.exe",
        ))
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-EncodedCommand",
        ])
        .arg(base64(&encoded))
        .output()
        .map_err(|err| format!("Could not start Windows speech recognition: {err}"))?;
        if !output.status.success() {
            return Err(
                "Windows speech recognition is not available. Add a speech language in Settings > Time & language > Speech, or use an OpenAI-compatible endpoint."
                    .into(),
            );
        }
        let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if text.is_empty() {
            return Err("Nothing was heard.".into());
        }
        Ok(text)
    }

    fn speak_local(&self, text: &str) -> Result<()> {
        self.stop_speech.store(false, Ordering::Relaxed);
        // SAFETY: a COM object on this thread, released before uninitialising.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let result = (|| -> windows_core::Result<()> {
                let voice: ISpVoice = CoCreateInstance(&SpVoice, None, CLSCTX_ALL)?;
                voice.Speak(&HSTRING::from(text), SPF_ASYNC, None)?;
                // A little at a time, so talking over it can stop it. Called through the table because the wrapper
                // reads S_FALSE, "not done yet", as success.
                let wait = |milliseconds: u32| {
                    (Interface::vtable(&voice).WaitUntilDone)(voice.as_raw(), milliseconds)
                };
                while wait(100) == S_FALSE {
                    if self.stop_speech.load(Ordering::Relaxed) {
                        return voice.Speak(
                            &HSTRING::from(""),
                            SPF_ASYNC | SPF_PURGEBEFORESPEAK,
                            None,
                        );
                    }
                }
                Ok(())
            })();
            CoUninitialize();
            result.map_err(|err| format!("Windows could not speak: {}", err.message()))
        }
    }

    fn stop_speaking(&self) {
        self.stop_speech.store(true, Ordering::Relaxed);
    }

    fn hears_phrases(&self) -> bool {
        self.availability().is_available()
    }

    fn wait_for_phrase(&self, phrase: &str, stop: &AtomicBool) -> Result<bool> {
        if let Some(why) = self.availability().reason() {
            return Err(why.to_owned());
        }
        let encoded: Vec<u8> = wake_script(phrase)
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let mut child = {
            use std::os::windows::process::CommandExt;
            std::process::Command::new(crate::system::system32(
                "WindowsPowerShell\\v1.0\\powershell.exe",
            ))
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-EncodedCommand",
            ])
            .arg(base64(&encoded))
            .creation_flags(0x0800_0000)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|err| format!("Could not start Windows speech recognition: {err}"))?
        };
        loop {
            if stop.load(Ordering::Relaxed) {
                // The microphone closes with it.
                let _ = child.kill();
                let _ = child.wait();
                return Ok(false);
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    let mut heard = String::new();
                    if let Some(mut out) = child.stdout.take() {
                        use std::io::Read;
                        let _ = out.read_to_string(&mut heard);
                    }
                    if heard.contains("heard") {
                        return Ok(true);
                    }
                    return Err(if status.success() {
                        "Listening for Hey Keenu stopped.".to_owned()
                    } else {
                        "Windows speech recognition is not available. Add a speech language in Settings > Time & language > Speech.".to_owned()
                    });
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(100)),
                Err(err) => return Err(err.to_string()),
            }
        }
    }
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len() * 4 / 3 + 4);
    for chunk in bytes.chunks(3) {
        let bits = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(TABLE[(bits >> 18) as usize & 63] as char);
        out.push(TABLE[(bits >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(bits >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[bits as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Plays a WAV in memory, waiting until it is done.
pub fn play_wav(wav: &[u8]) -> Result<()> {
    use windows::Win32::Media::Audio::{PlaySoundW, SND_MEMORY, SND_NODEFAULT, SND_SYNC};
    // SAFETY: `wav` outlives the synchronous call; the flag says the "name" is the sound itself.
    let played = unsafe {
        PlaySoundW(
            PCWSTR(wav.as_ptr().cast()),
            None,
            SND_MEMORY | SND_SYNC | SND_NODEFAULT,
        )
    };
    if played.as_bool() {
        Ok(())
    } else {
        Err("Windows could not play the voice's audio.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_the_script_as_powershell_wants() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
    }

    #[test]
    fn device_listing_starts_with_the_default_and_nothing_is_recording() {
        let voice = Voice::default();
        let devices = voice.devices().unwrap();
        assert!(devices[0].is_default);
        assert!(!voice.recording());
    }

    #[test]
    fn stopping_without_starting_is_an_error() {
        assert!(Voice::default().stop().is_err());
    }
}
