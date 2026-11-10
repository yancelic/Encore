//! SQLite clip library index, FFmpeg metadata/thumbnail/waveform extraction, and search/filter queries.
use std::collections::HashSet;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::UNIX_EPOCH;

use encore_common::Config;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const WAVEFORM_BUCKETS: usize = 240;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipRecord {
    pub id: String,
    pub path: String,
    pub title: String,
    pub game: String,
    pub tag: String,
    pub description: String,
    pub favorite: bool,
    pub duration_ms: u64,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub audio_tracks: u32,
    pub size_bytes: u64,
    pub created_at: i64,
    pub thumb_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortOrder {
    Newest,
    Oldest,
    Longest,
    FavoritesFirst,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClipQuery {
    pub search: String,
    pub game: Option<String>,
    pub favorites_only: bool,
    pub sort: Option<SortOrder>,
}

pub struct ClipStore {
    conn: Connection,
    thumbs_dir: PathBuf,
    waveforms_dir: PathBuf,
}

fn stable_id(path: &Path) -> String {
    // 64-bit FNV-1a over normalized lowercase path for deterministic cache/DB keys.
    let s = path.to_string_lossy().to_lowercase();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[derive(Debug)]
struct ProbedMedia {
    duration_ms: u64,
    width: u32,
    height: u32,
    fps: f64,
    audio_tracks: u32,
    game: Option<String>,
}

fn parse_duration_ms(line: &str) -> Option<u64> {
    let idx = line.find("Duration:")?;
    let rest = line[idx + 9..].trim_start();
    let ts = rest.split(',').next()?.trim();
    let mut parts = ts.split(':');
    let h: f64 = parts.next()?.parse().ok()?;
    let m: f64 = parts.next()?.parse().ok()?;
    let s: f64 = parts.next()?.parse().ok()?;
    Some(((h * 3600.0 + m * 60.0 + s) * 1000.0).round() as u64)
}

fn parse_video_dims_fps(line: &str) -> (u32, u32, f64) {
    let mut width = 0u32;
    let mut height = 0u32;
    let mut fps = 60.0f64;
    for tok in line.split(|c: char| c == ',' || c.is_whitespace()) {
        if let Some((a, b)) = tok.split_once('x') {
            if let (Ok(w), Ok(h)) = (a.parse::<u32>(), b.parse::<u32>()) {
                if w >= 64 && h >= 64 {
                    width = w;
                    height = h;
                }
            }
        }
    }
    if let Some(pos) = line.find(" fps") {
        let before = line[..pos].trim();
        if let Some(num) = before.split(|c: char| c == ',' || c.is_whitespace()).next_back() {
            if let Ok(v) = num.parse::<f64>() {
                if v > 0.0 {
                    fps = v;
                }
            }
        }
    }
    (width, height, fps)
}

fn probe_media(ffmpeg: &Path, path: &Path) -> ProbedMedia {
    let out = Command::new(ffmpeg)
        .creation_flags(CREATE_NO_WINDOW)
        .args(["-hide_banner", "-i"])
        .arg(path)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output();

    let mut probed = ProbedMedia {
        duration_ms: 0,
        width: 0,
        height: 0,
        fps: 60.0,
        audio_tracks: 0,
        game: None,
    };

    let Ok(out) = out else { return probed };
    let text = String::from_utf8_lossy(&out.stderr);
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Duration:") {
            if let Some(ms) = parse_duration_ms(trimmed) {
                probed.duration_ms = ms;
            }
        } else if trimmed.contains("Video:") {
            let (w, h, fps) = parse_video_dims_fps(trimmed);
            probed.width = w;
            probed.height = h;
            probed.fps = fps;
        } else if trimmed.contains("Audio:") {
            probed.audio_tracks += 1;
        } else if let Some(pos) = trimmed.find("encore_game:") {
            let g = trimmed[pos + 12..].trim();
            if !g.is_empty() {
                probed.game = Some(g.to_string());
            }
        }
    }
    probed
}

fn infer_game_name(path: &Path, probed_game: Option<String>) -> String {
    if let Some(g) = probed_game {
        return g;
    }
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
    for (needle, label) in [
        ("valorant", "VALORANT"),
        ("league", "League of Legends"),
        ("cs2", "Counter-Strike 2"),
        ("fortnite", "Fortnite"),
        ("apex", "Apex Legends"),
        ("overwatch", "Overwatch"),
        ("rocket", "Rocket League"),
        ("gta", "GTA V"),
    ] {
        if name.contains(needle) {
            return label.to_string();
        }
    }
    "Desktop Clip Capture".to_string()
}

fn default_title(path: &Path, game: &str) -> String {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Clip");
    if game != "Desktop Clip Capture" {
        format!("{game} — {stem}")
    } else {
        stem.to_string()
    }
}

fn generate_thumbnail(ffmpeg: &Path, src: &Path, thumb: &Path, duration_ms: u64) -> bool {
    if let Some(parent) = thumb.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let seek_secs = if duration_ms > 3000 { 1.0 } else { (duration_ms as f64 / 3000.0).max(0.0) };
    let seek_str = format!("{seek_secs:.2}");
    let status = Command::new(ffmpeg)
        .creation_flags(CREATE_NO_WINDOW)
        .args(["-y", "-ss", &seek_str, "-i"])
        .arg(src)
        .args(["-vframes", "1", "-vf", "scale=480:-2", "-q:v", "4"])
        .arg(thumb)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    status.is_ok_and(|s| s.success()) && thumb.exists()
}

impl ClipStore {
    pub fn open_default() -> Result<Self, String> {
        let dir = Config::dir();
        Self::open_at(&dir.join("encore.db"), &dir.join("thumbs"), &dir.join("waveforms"))
    }

    pub fn open_at(db_path: &Path, thumbs_dir: &Path, waveforms_dir: &Path) -> Result<Self, String> {
        if let Some(p) = db_path.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        std::fs::create_dir_all(thumbs_dir).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(waveforms_dir).map_err(|e| e.to_string())?;

        let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS clips (
                id TEXT PRIMARY KEY,
                path TEXT NOT NULL UNIQUE,
                title TEXT NOT NULL,
                game TEXT NOT NULL,
                tag TEXT NOT NULL DEFAULT '',
                description TEXT NOT NULL DEFAULT '',
                favorite INTEGER NOT NULL DEFAULT 0,
                duration_ms INTEGER NOT NULL DEFAULT 0,
                width INTEGER NOT NULL DEFAULT 0,
                height INTEGER NOT NULL DEFAULT 0,
                fps REAL NOT NULL DEFAULT 60.0,
                audio_tracks INTEGER NOT NULL DEFAULT 1,
                size_bytes INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                thumb_path TEXT NOT NULL DEFAULT ''
            );",
        )
        .map_err(|e| e.to_string())?;
        Ok(Self {
            conn,
            thumbs_dir: thumbs_dir.to_path_buf(),
            waveforms_dir: waveforms_dir.to_path_buf(),
        })
    }

    /// Scan `clips_dir`, prune deleted files, and index/thumbnail any new or changed `.mp4` files.
    pub fn sync_dir(&self, clips_dir: &Path, ffmpeg: &Path) -> Result<(), String> {
        let _ = std::fs::create_dir_all(clips_dir);
        let mut disk_ids = HashSet::new();
        let mut files = Vec::new();
        if let Ok(rd) = std::fs::read_dir(clips_dir) {
            for entry in rd.flatten() {
                let p = entry.path();
                if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("mp4")) {
                    let fname = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if fname.ends_with(".joined.mp4") || fname.starts_with(".tmp_") {
                        continue;
                    }
                    files.push(p);
                }
            }
        }

        for path in &files {
            let id = stable_id(path);
            disk_ids.insert(id.clone());
            let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
            let size_bytes = meta.len();
            if size_bytes < 1024 {
                continue; // skip incomplete/empty files
            }
            let created_at = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);

            let existing: Option<(u64, String)> = self
                .conn
                .query_row(
                    "SELECT size_bytes, thumb_path FROM clips WHERE id = ?1",
                    params![&id],
                    |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, String>(1)?)),
                )
                .ok();

            let needs_probe = match existing {
                Some((prev_size, prev_thumb)) => {
                    prev_size != size_bytes || prev_thumb.is_empty() || !Path::new(&prev_thumb).exists()
                }
                None => true,
            };

            if needs_probe {
                let probed = probe_media(ffmpeg, path);
                let game = infer_game_name(path, probed.game);
                let title = default_title(path, &game);
                let thumb = self.thumbs_dir.join(format!("{id}.jpg"));
                let thumb_str = if generate_thumbnail(ffmpeg, path, &thumb, probed.duration_ms) {
                    thumb.to_string_lossy().into_owned()
                } else {
                    String::new()
                };
                self.conn
                    .execute(
                        "INSERT INTO clips (id, path, title, game, duration_ms, width, height, fps, audio_tracks, size_bytes, created_at, thumb_path)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                         ON CONFLICT(id) DO UPDATE SET
                           duration_ms = excluded.duration_ms,
                           width = excluded.width,
                           height = excluded.height,
                           fps = excluded.fps,
                           audio_tracks = excluded.audio_tracks,
                           size_bytes = excluded.size_bytes,
                           thumb_path = excluded.thumb_path",
                        params![
                            &id,
                            path.to_string_lossy().as_ref(),
                            &title,
                            &game,
                            probed.duration_ms as i64,
                            probed.width as i64,
                            probed.height as i64,
                            probed.fps,
                            probed.audio_tracks.max(1) as i64,
                            size_bytes as i64,
                            created_at,
                            &thumb_str,
                        ],
                    )
                    .map_err(|e| e.to_string())?;
            }
        }

        // Prune DB rows whose files were removed outside Encore.
        let mut stmt = self.conn.prepare("SELECT id, path FROM clips").map_err(|e| e.to_string())?;
        let rows: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();
        drop(stmt);
        for (id, p) in rows {
            if !disk_ids.contains(&id) || !Path::new(&p).exists() {
                let _ = self.conn.execute("DELETE FROM clips WHERE id = ?1", params![&id]);
                let _ = std::fs::remove_file(self.thumbs_dir.join(format!("{id}.jpg")));
                let _ = std::fs::remove_file(self.waveforms_dir.join(format!("{id}.json")));
            }
        }
        Ok(())
    }

    pub fn list(&self, q: &ClipQuery) -> Result<Vec<ClipRecord>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, path, title, game, tag, description, favorite, duration_ms, width, height, fps, audio_tracks, size_bytes, created_at, thumb_path
                 FROM clips",
            )
            .map_err(|e| e.to_string())?;
        let mut items: Vec<ClipRecord> = stmt
            .query_map([], |r| {
                Ok(ClipRecord {
                    id: r.get(0)?,
                    path: r.get(1)?,
                    title: r.get(2)?,
                    game: r.get(3)?,
                    tag: r.get(4)?,
                    description: r.get(5)?,
                    favorite: r.get::<_, i64>(6)? != 0,
                    duration_ms: r.get::<_, i64>(7)? as u64,
                    width: r.get::<_, i64>(8)? as u32,
                    height: r.get::<_, i64>(9)? as u32,
                    fps: r.get(10)?,
                    audio_tracks: r.get::<_, i64>(11)? as u32,
                    size_bytes: r.get::<_, i64>(12)? as u64,
                    created_at: r.get(13)?,
                    thumb_path: r.get(14)?,
                })
            })
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();

        let search = q.search.trim().to_lowercase();
        items.retain(|c| {
            if q.favorites_only && !c.favorite {
                return false;
            }
            if let Some(ref g) = q.game {
                if !g.is_empty() && !g.eq_ignore_ascii_case("all") && !c.game.eq_ignore_ascii_case(g) {
                    return false;
                }
            }
            if !search.is_empty() {
                let hay = format!("{} {} {} {}", c.title, c.game, c.tag, c.description).to_lowercase();
                if !hay.contains(&search) {
                    return false;
                }
            }
            true
        });

        match q.sort.unwrap_or(SortOrder::Newest) {
            SortOrder::Newest => items.sort_by_key(|b| std::cmp::Reverse(b.created_at)),
            SortOrder::Oldest => items.sort_by_key(|a| a.created_at),
            SortOrder::Longest => items.sort_by_key(|b| std::cmp::Reverse(b.duration_ms)),
            SortOrder::FavoritesFirst => {
                items.sort_by_key(|b| std::cmp::Reverse((b.favorite, b.created_at)))
            }
        }
        Ok(items)
    }

    pub fn get(&self, id: &str) -> Result<ClipRecord, String> {
        self.conn
            .query_row(
                "SELECT id, path, title, game, tag, description, favorite, duration_ms, width, height, fps, audio_tracks, size_bytes, created_at, thumb_path
                 FROM clips WHERE id = ?1",
                params![id],
                |r| {
                    Ok(ClipRecord {
                        id: r.get(0)?,
                        path: r.get(1)?,
                        title: r.get(2)?,
                        game: r.get(3)?,
                        tag: r.get(4)?,
                        description: r.get(5)?,
                        favorite: r.get::<_, i64>(6)? != 0,
                        duration_ms: r.get::<_, i64>(7)? as u64,
                        width: r.get::<_, i64>(8)? as u32,
                        height: r.get::<_, i64>(9)? as u32,
                        fps: r.get(10)?,
                        audio_tracks: r.get::<_, i64>(11)? as u32,
                        size_bytes: r.get::<_, i64>(12)? as u64,
                        created_at: r.get(13)?,
                        thumb_path: r.get(14)?,
                    })
                },
            )
            .map_err(|e| e.to_string())
    }

    pub fn set_favorite(&self, id: &str, fav: bool) -> Result<(), String> {
        self.conn
            .execute("UPDATE clips SET favorite = ?1 WHERE id = ?2", params![fav as i64, id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn update_details(&self, id: &str, title: &str, tag: &str, description: &str) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE clips SET title = ?1, tag = ?2, description = ?3 WHERE id = ?4",
                params![title, tag, description, id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        if let Ok(rec) = self.get(id) {
            let _ = std::fs::remove_file(&rec.path);
            if !rec.thumb_path.is_empty() {
                let _ = std::fs::remove_file(&rec.thumb_path);
            }
        }
        let _ = std::fs::remove_file(self.waveforms_dir.join(format!("{id}.json")));
        self.conn
            .execute("DELETE FROM clips WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Extract normalized peak amplitudes (`[0.0..1.0]`, 240 buckets) per audio track, cached on disk.
    pub fn get_waveforms(&self, id: &str, ffmpeg: &Path) -> Result<Vec<Vec<f32>>, String> {
        let cache = self.waveforms_dir.join(format!("{id}.json"));
        if let Ok(raw) = std::fs::read_to_string(&cache) {
            if let Ok(parsed) = serde_json::from_str::<Vec<Vec<f32>>>(&raw) {
                return Ok(parsed);
            }
        }
        let rec = self.get(id)?;
        let count = rec.audio_tracks.clamp(1, 3);
        let mut tracks = Vec::with_capacity(count as usize);
        for t in 0..count {
            tracks.push(extract_track_peaks(ffmpeg, Path::new(&rec.path), t));
        }
        let _ = std::fs::write(&cache, serde_json::to_string(&tracks).unwrap_or_default());
        Ok(tracks)
    }
}

fn extract_track_peaks(ffmpeg: &Path, src: &Path, track_idx: u32) -> Vec<f32> {
    let map = format!("0:a:{track_idx}");
    let out = Command::new(ffmpeg)
        .creation_flags(CREATE_NO_WINDOW)
        .args(["-i"])
        .arg(src)
        .args(["-map", &map, "-ac", "1", "-ar", "16000", "-f", "s16le", "pipe:1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();
    let Ok(out) = out else { return vec![0.0; WAVEFORM_BUCKETS] };
    if !out.status.success() || out.stdout.len() < 4 {
        return vec![0.0; WAVEFORM_BUCKETS];
    }
    let samples: Vec<i16> = out
        .stdout
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect();
    let total = samples.len();
    let mut peaks = Vec::with_capacity(WAVEFORM_BUCKETS);
    for b in 0..WAVEFORM_BUCKETS {
        let start = (b * total) / WAVEFORM_BUCKETS;
        let end = (((b + 1) * total) / WAVEFORM_BUCKETS).max(start + 1).min(total);
        let max_amp = samples[start..end]
            .iter()
            .map(|&s| (s as i32).unsigned_abs())
            .max()
            .unwrap_or(0);
        peaks.push((max_amp as f32 / 32768.0).min(1.0));
    }
    peaks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ffmpeg() -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("tools").join("ffmpeg.exe");
        assert!(root.exists(), "missing tools/ffmpeg.exe for clipstore test");
        root
    }

    #[test]
    fn indexes_clip_generates_thumb_waveforms_and_persists_details() {
        let ff = test_ffmpeg();
        let dir = std::env::temp_dir().join(format!("encore_store_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let clips_dir = dir.join("clips");
        std::fs::create_dir_all(&clips_dir).unwrap();
        let clip_path = clips_dir.join("Encore_test_clip.mp4");

        // Create a 2-second 3-track synthetic MP4 using FFmpeg lavfi.
        let st = Command::new(&ff)
            .args([
                "-y",
                "-f", "lavfi", "-i", "color=c=teal:s=640x360:r=30:d=2",
                "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=2",
                "-f", "lavfi", "-i", "anullsrc=r=48000:cl=stereo:d=2",
                "-f", "lavfi", "-i", "sine=frequency=880:sample_rate=48000:duration=2",
                "-map", "0:v", "-map", "1:a", "-map", "2:a", "-map", "3:a",
                "-c:v", "libx264", "-c:a", "aac",
                "-metadata", "comment=encore_game:VALORANT",
            ])
            .arg(&clip_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(st.success());

        let store = ClipStore::open_at(&dir.join("encore.db"), &dir.join("thumbs"), &dir.join("waves")).unwrap();
        store.sync_dir(&clips_dir, &ff).unwrap();

        let items = store.list(&ClipQuery::default()).unwrap();
        assert_eq!(items.len(), 1);
        let rec = &items[0];
        assert_eq!(rec.game, "VALORANT");
        assert_eq!(rec.audio_tracks, 3);
        assert_eq!(rec.width, 640);
        assert_eq!(rec.height, 360);
        assert!(rec.duration_ms >= 1900 && rec.duration_ms <= 2200);
        assert!(Path::new(&rec.thumb_path).exists());

        store.set_favorite(&rec.id, true).unwrap();
        store.update_details(&rec.id, "Ace on Ascent", "Ace", "4k vandal clutch").unwrap();
        let filtered = store
            .list(&ClipQuery {
                search: "ascent".into(),
                game: Some("VALORANT".into()),
                favorites_only: true,
                sort: Some(SortOrder::Newest),
            })
            .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].tag, "Ace");

        let waves = store.get_waveforms(&rec.id, &ff).unwrap();
        assert_eq!(waves.len(), 3);
        assert_eq!(waves[0].len(), WAVEFORM_BUCKETS);
        assert!(waves[0].iter().copied().fold(0.0f32, f32::max) > 0.1);
        assert!(waves[1].iter().copied().fold(0.0f32, f32::max) < 0.01); // silent middle track
        assert!(waves[2].iter().copied().fold(0.0f32, f32::max) > 0.1);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
