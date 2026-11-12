//! Shared configuration, IPC types, and platform services for Encore.
#[cfg(windows)]
#[path = "platform/windows/autostart.rs"]
pub mod autostart;
#[cfg(not(windows))]
#[path = "platform/portable/autostart.rs"]
pub mod autostart;
pub mod ipc;
#[cfg(windows)]
#[path = "platform/windows/processes.rs"]
pub mod processes;
#[cfg(not(windows))]
#[path = "platform/portable/processes.rs"]
pub mod processes;
pub mod platform;
pub mod tray_icon;

use global_hotkey::hotkey::HotKey;
use global_hotkey::hotkey::Code;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::str::FromStr;

/// Capture target selection mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaptureMode {
    /// Record automatically when a known game process is running.
    GameDetection,
    /// Record whichever window is focused.
    ActiveWindow,
}

/// Device routing remains the default for compatibility with existing settings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioMode {
    #[default]
    Devices,
    Applications,
}

/// User-selectable settings, stored as JSON in the platform's configuration directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub mode: CaptureMode,
    pub fps: u32,
    pub height: u32,
    /// Length of the saved clip in seconds.
    pub buffer_secs: u32,
    /// Global hotkey that saves a clip, e.g. "Alt+S".
    pub hotkey: String,
    pub audio_mode: AudioMode,
    /// In application mode, capture the selected game's process tree.
    pub game_audio_enabled: bool,
    /// Executable filename, never a window title or persisted PID. None = chat off.
    pub chat_application: Option<String>,
    /// Track 0 (Game): substring of output device name (`None` = default device, `"off"` = silent).
    pub audio_device: Option<String>,
    /// Track 1 (Chat): substring of output device name (`None` or `"off"` = silent, `"default"` = default output).
    pub chat_device: Option<String>,
    /// Track 2 (Microphone): substring of input device name (`None` or `"off"` = silent, `"default"` = default mic).
    pub mic_device: Option<String>,
    /// Whether the recorder starts automatically with Windows.
    pub start_with_windows: bool,
    /// Where finished clips are stored.
    pub clips_dir: PathBuf,
    /// Executable names that count as games in `GameDetection` mode (case-insensitive).
    pub games: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        let videos = platform::videos_dir();
        Self {
            mode: CaptureMode::GameDetection,
            fps: 60,
            height: 1080,
            buffer_secs: 60,
            hotkey: "Alt+S".into(),
            audio_mode: AudioMode::Devices,
            game_audio_enabled: true,
            chat_application: None,
            audio_device: None,
            chat_device: None,
            mic_device: Some("default".into()),
            start_with_windows: false,
            clips_dir: videos.join("Encore"),
            games: default_games(),
        }
    }
}

/// Map a process executable name to a friendly display name for clip grouping.
pub fn pretty_game_name(exe: &str) -> String {
    let lower = exe.to_ascii_lowercase();
    match lower.as_str() {
        "valorant-win64-shipping.exe" | "valorant.exe" => "VALORANT".into(),
        "league of legends.exe" => "League of Legends".into(),
        "cs2.exe" => "Counter-Strike 2".into(),
        "fortniteclient-win64-shipping.exe" => "Fortnite".into(),
        "r5apex.exe" => "Apex Legends".into(),
        "overwatch.exe" => "Overwatch".into(),
        "rocketleague.exe" => "Rocket League".into(),
        "gta5.exe" => "GTA V".into(),
        _ => exe.strip_suffix(".exe").or_else(|| exe.strip_suffix(".EXE")).unwrap_or(exe).to_string(),
    }
}

/// Games recognised out of the box; the UI can edit this list.
pub fn default_games() -> Vec<String> {
    [
        "VALORANT-Win64-Shipping.exe",
        "League of Legends.exe",
        "cs2.exe",
        "FortniteClient-Win64-Shipping.exe",
        "r5apex.exe",
        "Overwatch.exe",
        "RocketLeague.exe",
        "GTA5.exe",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

impl Config {
    pub fn validate_audio(&self) -> Result<(), String> {
        if self.audio_mode == AudioMode::Applications {
            if !processes::application_loopback_supported() {
                return Err("Application audio requires Windows build 20348 or later. Select Devices instead.".into());
            }
            if self.chat_application.as_deref().is_some_and(|exe| !processes::valid_executable(exe)) {
                return Err("Select a chat executable filename such as Chat.exe, not a window title or file path.".into());
            }
        }
        Ok(())
    }

    pub fn dir() -> PathBuf {
        platform::config_dir()
    }

    pub fn path() -> PathBuf {
        Self::dir().join("config.json")
    }

    /// Load the config, writing the defaults on first run.
    pub fn load() -> Self {
        match std::fs::read_to_string(Self::path()) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => {
                let c = Self::default();
                let _ = c.save();
                c
            }
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(Self::dir())?;
        std::fs::write(Self::path(), serde_json::to_string_pretty(self).unwrap())
    }
}

/// Compute even encoded dimensions from source size and `target_height` (`0` or `>= src_h` = native).
pub fn scale_dimensions(src_w: u32, src_h: u32, target_height: u32) -> (u32, u32) {
    let sw = (src_w & !1).max(2);
    let sh = (src_h & !1).max(2);
    if target_height == 0 || target_height >= sh {
        return (sw, sh);
    }
    let oh = (target_height & !1).max(2);
    let ow = ((((sw as u64) * (oh as u64) + (sh as u64 / 2)) / (sh as u64)) as u32 & !1).max(2);
    (ow, oh)
}

/// Scale video bitrate proportionally to pixel rate around a 1080p60 = 20 Mbps baseline.
pub fn target_bitrate(out_w: u32, out_h: u32, fps: u32) -> u32 {
    let px_per_sec = (out_w as u64) * (out_h as u64) * (fps.max(1) as u64);
    let base_px = 1920u64 * 1080 * 60;
    let bps = (px_per_sec * 20_000_000) / base_px;
    bps.clamp(8_000_000, 80_000_000) as u32
}

/// Locate FFmpeg in a bundle, development checkout, or PATH.
pub fn ffmpeg_path() -> PathBuf {
    platform::ffmpeg_path()
}

fn is_supported_key(code: Code) -> bool {
    matches!(
        code,
        Code::KeyA
            | Code::KeyB
            | Code::KeyC
            | Code::KeyD
            | Code::KeyE
            | Code::KeyF
            | Code::KeyG
            | Code::KeyH
            | Code::KeyI
            | Code::KeyJ
            | Code::KeyK
            | Code::KeyL
            | Code::KeyM
            | Code::KeyN
            | Code::KeyO
            | Code::KeyP
            | Code::KeyQ
            | Code::KeyR
            | Code::KeyS
            | Code::KeyT
            | Code::KeyU
            | Code::KeyV
            | Code::KeyW
            | Code::KeyX
            | Code::KeyY
            | Code::KeyZ
            | Code::Digit0
            | Code::Digit1
            | Code::Digit2
            | Code::Digit3
            | Code::Digit4
            | Code::Digit5
            | Code::Digit6
            | Code::Digit7
            | Code::Digit8
            | Code::Digit9
            | Code::Equal
            | Code::Comma
            | Code::Minus
            | Code::Period
            | Code::Semicolon
            | Code::Slash
            | Code::Backquote
            | Code::BracketLeft
            | Code::Backslash
            | Code::BracketRight
            | Code::Quote
            | Code::IntlBackslash
            | Code::IntlRo
            | Code::IntlYen
            | Code::Backspace
            | Code::Tab
            | Code::Space
            | Code::Enter
            | Code::CapsLock
            | Code::ContextMenu
            | Code::Convert
            | Code::KanaMode
            | Code::NonConvert
            | Code::Escape
            | Code::PageUp
            | Code::PageDown
            | Code::End
            | Code::Home
            | Code::ArrowLeft
            | Code::ArrowUp
            | Code::ArrowRight
            | Code::ArrowDown
            | Code::PrintScreen
            | Code::Insert
            | Code::Delete
            | Code::Help
            | Code::F1
            | Code::F2
            | Code::F3
            | Code::F4
            | Code::F5
            | Code::F6
            | Code::F7
            | Code::F8
            | Code::F9
            | Code::F10
            | Code::F11
            | Code::F12
            | Code::F13
            | Code::F14
            | Code::F15
            | Code::F16
            | Code::F17
            | Code::F18
            | Code::F19
            | Code::F20
            | Code::F21
            | Code::F22
            | Code::F23
            | Code::F24
            | Code::NumLock
            | Code::Numpad0
            | Code::Numpad1
            | Code::Numpad2
            | Code::Numpad3
            | Code::Numpad4
            | Code::Numpad5
            | Code::Numpad6
            | Code::Numpad7
            | Code::Numpad8
            | Code::Numpad9
            | Code::NumpadAdd
            | Code::NumpadClear
            | Code::NumpadComma
            | Code::NumpadDecimal
            | Code::NumpadDivide
            | Code::NumpadEnter
            | Code::NumpadEqual
            | Code::NumpadMultiply
            | Code::NumpadSubtract
            | Code::ScrollLock
            | Code::BrowserBack
            | Code::BrowserFavorites
            | Code::BrowserForward
            | Code::BrowserHome
            | Code::BrowserRefresh
            | Code::BrowserSearch
            | Code::BrowserStop
            | Code::LaunchApp1
            | Code::LaunchApp2
            | Code::LaunchMail
            | Code::MediaSelect
            | Code::Sleep
            | Code::AudioVolumeDown
            | Code::AudioVolumeUp
            | Code::AudioVolumeMute
            | Code::MediaPlay
            | Code::MediaPause
            | Code::MediaPlayPause
            | Code::MediaStop
            | Code::MediaTrackNext
            | Code::MediaTrackPrevious
            | Code::Pause
    )
}

/// Parse a global hotkey, applying RegisterHotKey restrictions on Windows.
pub fn parse_hotkey(s: &str) -> Result<HotKey, String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err("Hotkey cannot be empty".into());
    }
    let hk = HotKey::from_str(trimmed).map_err(|_| {
        format!("Invalid hotkey '{trimmed}'. Please pick a valid key combination in Settings.")
    })?;
    if !is_supported_key(hk.key) {
        return Err(format!(
            "Unsupported hotkey key '{}' in '{trimmed}'. Please pick another hotkey in Settings.",
            hk.key
        ));
    }
    Ok(hk)
}

pub fn validate_hotkey(s: &str) -> Result<(), String> {
    parse_hotkey(s).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_dimensions_preserves_aspect_and_even_alignment() {
        assert_eq!(scale_dimensions(1920, 1200, 0), (1920, 1200));
        assert_eq!(scale_dimensions(1920, 1200, 1440), (1920, 1200)); // never upscale
        assert_eq!(scale_dimensions(1920, 1200, 1080), (1728, 1080));
        assert_eq!(scale_dimensions(1920, 1200, 720), (1152, 720));
        assert_eq!(scale_dimensions(1920, 1080, 720), (1280, 720));
        assert_eq!(scale_dimensions(2560, 1440, 1080), (1920, 1080));
        assert_eq!(scale_dimensions(1919, 1079, 0), (1918, 1078));
    }

    #[test]
    fn target_bitrate_scales_within_bounds() {
        assert_eq!(target_bitrate(1920, 1080, 60), 20_000_000);
        assert_eq!(target_bitrate(1280, 720, 30), 8_000_000);
        assert_eq!(target_bitrate(1920, 1080, 240), 80_000_000);
    }

    #[test]
    fn default_hotkey_is_alt_s_and_roundtrips() {
        let c = Config::default();
        assert_eq!(c.hotkey, "Alt+S");
        let back: Config = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back.hotkey, "Alt+S");
    }

    #[test]
    fn default_games_include_valorant() {
        assert!(Config::default().games.iter().any(|g| g.eq_ignore_ascii_case("valorant-win64-shipping.exe")));
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let c: Config = serde_json::from_str(r#"{"hotkey":"Ctrl+F9"}"#).unwrap();
        assert_eq!(c.hotkey, "Ctrl+F9");
        assert_eq!(c.fps, 60);
        assert_eq!(c.audio_mode, AudioMode::Devices);
        assert!(c.game_audio_enabled);
        assert!(c.chat_application.is_none());
    }

    #[test]
    fn parse_hotkey_validates_supported_and_rejects_invalid_keys() {
        assert!(validate_hotkey("Alt+S").is_ok());
        assert!(validate_hotkey("Ctrl+Alt+K").is_ok());
        assert!(validate_hotkey("Alt+Backquote").is_ok());
        assert!(validate_hotkey("F9").is_ok());
        assert!(validate_hotkey("Alt+\"").is_err());
        assert!(validate_hotkey("").is_err());
        assert!(validate_hotkey("Alt+F25").is_err());
    }
}
