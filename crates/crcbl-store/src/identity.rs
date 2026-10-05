//! The client's [`PlayerId`]: drawn once, kept where the platform keeps small
//! things, and presented in every hello after.
//!
//! ```text
//!  native, windowed  ──▶ ~/.config/<app>/player.id   write_atomic
//!  native, headless  ──▶ nowhere                     PlayerId::from_seed
//!  wasm32            ──▶ the Origin Private File System
//!  no store at all   ──▶ nowhere                     a fresh id each run
//! ```
//!
//! The same [`Backing`] a [`Record`](crate::record::Record) is kept in, for the
//! same reasons: the platform's arms are a fact about the platform, and a
//! headless run must leave nothing behind in whoever's config directory it ran
//! as. Unlike a record, **an id that cannot be read is never written over**:
//! a store whose read failed, a browser store still restoring, or a file from
//! a newer engine each refuse with an [`IdentityError`], because writing a
//! fresh id there would make the player a stranger to every server that knew
//! them. Only a file that is plainly damaged — the wrong length or the wrong
//! magic — is replaced, logged.
//!
//! The id is self-asserted, and `PlayerId`'s docs say what that leaves open;
//! keeping it in a file the player can read and copy changes nothing there.

// Only the browser arms name a path: natively the file is a `PathBuf` joined
// onto the backing's root.
#[cfg(target_arch = "wasm32")]
use std::path::Path;

use crcbl_core::PlayerId;

use crate::StorageError;
use crate::record::Backing;

/// The file the id is kept in, under the backing's root.
pub const PLAYER_ID_FILE: &str = "player.id";

/// The seed of the id a headless run presents: the same every run, so a
/// headless test or CI job behaves alike on every machine. Two headless
/// clients of one server would be one player to it; a test that joins
/// several picks its own seeds with [`PlayerId::from_seed`].
pub const HEADLESS_PLAYER_SEED: u64 = 0x4845_4144_4C45_5353;

/// What the file starts with: what it is, before anything reads its id.
const MAGIC: [u8; 4] = *b"CPID";

/// The file's format version, after the magic. A file of a later version is
/// refused rather than overwritten ([`IdentityError::NewerFormat`]): it may
/// carry what an authenticated tier keeps beside the id.
const FORMAT_VERSION: u8 = 1;

/// The file's whole length at [`FORMAT_VERSION`]: the magic, the version and
/// the id.
const FILE_BYTES: usize = MAGIC.len() + 1 + PlayerId::BYTES;

/// Why no id could be had.
#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    /// There was no id to read and none could be drawn.
    #[error("no player id could be drawn: {0}")]
    Entropy(crcbl_rand::Error),
    /// The file is there and would not be read. It is left as it is.
    #[error("the player id could not be read: {0}")]
    Read(StorageError),
    /// The browser store has not finished restoring, so whether an id is in
    /// it is not known yet. Ask again once it has.
    #[error("the browser store is still restoring; the player id is not resident yet")]
    NotResident,
    /// The file is from a newer engine, of this format version. It is left
    /// as it is.
    #[error("the player id file is format version {0}, newer than this build reads")]
    NewerFormat(u8),
}

/// The id `app`'s client presents: [`load_or_create`] in the platform's
/// store ([`Backing::platform`]), or — for a `headless` run, which must
/// leave nothing behind — [`HEADLESS_PLAYER_SEED`]'s.
///
/// # Errors
///
/// [`load_or_create`]'s.
pub fn for_app(app: &str, headless: bool) -> Result<PlayerId, IdentityError> {
    if headless {
        return Ok(PlayerId::from_seed(HEADLESS_PLAYER_SEED));
    }
    load_or_create(&Backing::platform(app))
}

/// The id kept in `backing`, or a new one drawn from the secure entropy
/// source and kept there for next time. [`Backing::None`] keeps nothing: each
/// call draws a fresh id, the player a new one each run.
///
/// A new id that cannot be written is still returned, and logged: it serves
/// this run, and the next run draws another.
///
/// # Errors
///
/// [`IdentityError`]: no id could be drawn, or one may be there and could not
/// be read — see the [module docs](self) for why that is never written over.
pub fn load_or_create(backing: &Backing) -> Result<PlayerId, IdentityError> {
    if let Some(bytes) = read(backing)? {
        match decode(&bytes)? {
            Some(id) => return Ok(id),
            None => crcbl_core::log::warn!(
                "identity: {PLAYER_ID_FILE} is damaged ({} bytes); drawing a new player id",
                bytes.len()
            ),
        }
    }
    let id = draw()?;
    if let Err(error) = write(backing, id) {
        crcbl_core::log::warn!("identity: the new player id was not kept ({error})");
    }
    Ok(id)
}

/// The file's bytes, `None` when there is none to read.
fn read(backing: &Backing) -> Result<Option<Vec<u8>>, IdentityError> {
    let read = match backing {
        Backing::None => return Ok(None),
        #[cfg(not(target_arch = "wasm32"))]
        Backing::Native(root) => {
            let path = root.join(PLAYER_ID_FILE);
            std::fs::read(&path).map_err(|error| StorageError::from_io(&path, error))
        }
        #[cfg(target_arch = "wasm32")]
        Backing::Browser(store) => store.read(Path::new(PLAYER_ID_FILE)),
    };
    match read {
        Ok(bytes) => Ok(Some(bytes)),
        Err(StorageError::NotFound(_)) => Ok(None),
        Err(StorageError::Pending(_)) => Err(IdentityError::NotResident),
        Err(error) => Err(IdentityError::Read(error)),
    }
}

/// The id `bytes` hold, `None` when they are damaged.
fn decode(bytes: &[u8]) -> Result<Option<PlayerId>, IdentityError> {
    let Some(rest) = bytes.strip_prefix(&MAGIC) else {
        return Ok(None);
    };
    match rest.split_first() {
        Some((&version, _)) if version > FORMAT_VERSION => Err(IdentityError::NewerFormat(version)),
        Some((&FORMAT_VERSION, id)) => Ok(<[u8; PlayerId::BYTES]>::try_from(id)
            .ok()
            .map(PlayerId::from_bytes)),
        _ => Ok(None),
    }
}

/// `id` as the file holds it.
fn encode(id: PlayerId) -> [u8; FILE_BYTES] {
    let mut bytes = [0u8; FILE_BYTES];
    bytes[..MAGIC.len()].copy_from_slice(&MAGIC);
    bytes[MAGIC.len()] = FORMAT_VERSION;
    bytes[MAGIC.len() + 1..].copy_from_slice(&id.to_bytes());
    bytes
}

/// A new id from the secure entropy source.
fn draw() -> Result<PlayerId, IdentityError> {
    let mut bytes = [0u8; PlayerId::BYTES];
    crcbl_rand::entropy(&mut bytes).map_err(IdentityError::Entropy)?;
    Ok(PlayerId::from_bytes(bytes))
}

/// Keep `id` in `backing`, if it keeps anything.
fn write(backing: &Backing, id: PlayerId) -> Result<(), StorageError> {
    let bytes = encode(id);
    match backing {
        Backing::None => Ok(()),
        #[cfg(not(target_arch = "wasm32"))]
        Backing::Native(root) => crate::write_atomic(&root.join(PLAYER_ID_FILE), &bytes),
        #[cfg(target_arch = "wasm32")]
        Backing::Browser(store) => store.write(Path::new(PLAYER_ID_FILE), &bytes),
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    fn native(dir: &tempfile::TempDir) -> Backing {
        Backing::Native(dir.path().to_path_buf())
    }

    /// **The id survives a restart**: a second open of the same config
    /// directory — a new run of the client — reads back the id the first
    /// drew, and another directory, another player, draws its own.
    #[test]
    fn the_id_survives_a_restart_over_the_same_config() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let first = load_or_create(&native(&dir)).expect("an id is drawn");
        assert!(dir.path().join(PLAYER_ID_FILE).exists(), "nothing kept");
        let restarted = load_or_create(&native(&dir)).expect("the id reads back");
        assert_eq!(restarted, first, "a restart drew a new player id");

        let other = tempfile::tempdir().expect("a temp dir");
        assert_ne!(load_or_create(&native(&other)).expect("drawn"), first);
    }

    /// **A damaged file is replaced, and the new id kept** — the wrong length
    /// and the wrong magic each — rather than refusing to play.
    #[test]
    fn a_damaged_file_is_replaced_and_the_new_id_kept() {
        for damaged in [&[1u8, 2, 3][..], &[0u8; FILE_BYTES][..]] {
            let dir = tempfile::tempdir().expect("a temp dir");
            let path = dir.path().join(PLAYER_ID_FILE);
            std::fs::write(&path, damaged).expect("the temp dir is writable");
            let id = load_or_create(&native(&dir)).expect("a new id is drawn");
            assert_eq!(
                std::fs::read(&path).expect("kept"),
                encode(id),
                "the new id was not kept over {damaged:?}"
            );
        }
    }

    /// **A newer engine's file is refused and left alone**, so a downgrade
    /// cannot make the player a stranger to their servers.
    #[test]
    fn a_newer_engines_file_is_refused_and_left_alone() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let path = dir.path().join(PLAYER_ID_FILE);
        let mut newer = encode(PlayerId::from_seed(1)).to_vec();
        newer[MAGIC.len()] = FORMAT_VERSION + 1;
        std::fs::write(&path, &newer).expect("the temp dir is writable");
        assert!(matches!(
            load_or_create(&native(&dir)),
            Err(IdentityError::NewerFormat(version)) if version == FORMAT_VERSION + 1
        ));
        assert_eq!(std::fs::read(&path).expect("still there"), newer);
    }

    /// **A file that is there and will not read is left alone** — a
    /// directory where the file should be, which reads as an error that is
    /// not "not found".
    #[test]
    fn a_file_that_will_not_read_is_left_alone() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let path = dir.path().join(PLAYER_ID_FILE);
        std::fs::create_dir(&path).expect("the temp dir is writable");
        assert!(matches!(
            load_or_create(&native(&dir)),
            Err(IdentityError::Read(_))
        ));
        assert!(path.is_dir(), "the unreadable entry was replaced");
    }

    /// **A headless run presents the seeded id and writes nothing**, and a
    /// run with no store draws a fresh id each time.
    #[test]
    fn a_headless_run_presents_the_seeded_id_and_no_store_a_fresh_one() {
        assert_eq!(
            for_app("crcbl-identity-test", true).expect("headless needs no store"),
            PlayerId::from_seed(HEADLESS_PLAYER_SEED)
        );
        let once = load_or_create(&Backing::None).expect("drawn");
        let again = load_or_create(&Backing::None).expect("drawn");
        assert_ne!(once, again, "no store kept an id anyway");
    }
}
