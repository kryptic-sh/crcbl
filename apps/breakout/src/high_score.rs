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

    /// Load with a specific root directory (for testing).
    #[cfg(test)]
    fn with_root(root: std::path::PathBuf) -> Self {
        let path = root.join(HIGH_SCORE_FILE);
        let value = match std::fs::read(&path) {
            Ok(data) if data.len() == 4 => u32::from_le_bytes(data[..4].try_into().unwrap()),
            _ => 0,
        };
        Self {
            root: Some(root),
            value,
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_score_persists_and_reloads() {
        let dir = std::env::temp_dir().join("crcbl_breakout_test_hs");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");

        // First session: score 50, verify it saves.
        let mut hs = HighScore::with_root(dir.clone());
        assert_eq!(hs.get(), 0);
        assert!(hs.update(50));
        assert_eq!(hs.get(), 50);

        // Same score should not update.
        assert!(!hs.update(40));
        assert_eq!(hs.get(), 50);

        // Higher score should update.
        assert!(hs.update(100));
        assert_eq!(hs.get(), 100);

        // Second session: reload from disk, verify value persisted.
        let hs2 = HighScore::with_root(dir.clone());
        assert_eq!(hs2.get(), 100);

        // Clean up.
        let _ = std::fs::remove_dir_all(&dir);
    }
}
