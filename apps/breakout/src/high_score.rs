//! High-score persistence — saves a single u32 to `~/.config/breakout/`.
//!
//! Headless runs skip disk I/O entirely. Windowed builds use
//! `crcbl_store::write_atomic` for safe writes.

use std::path::PathBuf;

const HIGH_SCORE_FILE: &str = "high_score.bin";

/// Manages a single persistent high score value.
pub struct HighScore {
    /// Where to persist, or `None` for headless.
    root: Option<PathBuf>,
    value: u32,
}

impl HighScore {
    /// Load the high score. Headless runs are in-memory only.
    pub fn load(headless: bool) -> Self {
        let root = if headless {
            None
        } else {
            match crcbl_store::NativeStorage::config("breakout") {
                Ok(s) => Some(s.root().to_path_buf()),
                Err(_) => {
                    log::warn!("high_score: no config dir; scores will not persist");
                    None
                }
            }
        };

        let value = root
            .as_ref()
            .map(|r| r.join(HIGH_SCORE_FILE))
            .and_then(|path| match std::fs::read(&path) {
                Ok(data) if data.len() == 4 => {
                    Some(u32::from_le_bytes(data[..4].try_into().unwrap()))
                }
                Ok(data) => {
                    log::warn!("high_score: corrupt file ({} bytes)", data.len());
                    None
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    log::info!("high_score: no previous save");
                    None
                }
                Err(e) => {
                    log::warn!("high_score: read error ({e})");
                    None
                }
            })
            .unwrap_or(0);

        Self { root, value }
    }

    /// Current best score.
    pub fn get(&self) -> u32 {
        self.value
    }

    /// Update the high score if `score` exceeds it. Returns `true` on record.
    pub fn update(&mut self, score: u32) -> bool {
        if score > self.value {
            self.value = score;
            if let Some(ref root) = self.root {
                let path = root.join(HIGH_SCORE_FILE);
                if let Err(e) = crcbl_store::write_atomic(&path, &self.value.to_le_bytes()) {
                    log::warn!("high_score: save failed ({e})");
                }
            }
            log::info!("high_score: new best = {}", self.value);
            return true;
        }
        false
    }
}
