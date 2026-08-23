//! The `[engine.video]` layer of `docs/plan/39-capabilities.md`'s effect
//! resolution order: which of topic 18's effects the **player** allows.
//!
//! ```text
//! camera stack declares what the view wants
//!   → [engine.video] clamps it downward as a player quality setting   ← here
//!   → programmatic override may set it either way
//!   → device capability clamps it downward, last and absolutely
//! ```
//!
//! [`crcbl_store::settings`] is the mechanism — layered TOML, dotted keys,
//! typed reads — and [`crcbl_render::effects`] is the resolution point. This
//! module is the join between them, and it is here rather than in either
//! because neither may depend on the other: `crcbl-render` has no storage and
//! `crcbl-store` has no idea what an effect is.
//!
//! # Where the read happens
//!
//! [`GpuContext::open`](crate::engine::GpuContext::open) and its two siblings,
//! from [`SettingsSource`](crate::engine::SettingsSource) — so every sample and
//! every `crcbl new` scaffold reads the player's settings without asking, and
//! [`GpuContext::effect_request`](crate::engine::GpuContext::effect_request) is
//! what a renderer built on that context is handed. Nothing in that path is
//! fallible: a player with no settings file is the ordinary first run, not a
//! start-up that failed.
//!
//! # A key that is absent is not a key that says "off"
//!
//! This layer only ever **clamps downward**, so the question each key answers
//! is "has the player asked for *less*?" — and a file that does not mention an
//! effect has not. [`video_effects`] therefore starts from
//! [`RenderEffects::all`] and removes a bit only for a key that is present and
//! `false`; `true` and absent are the same answer, which is why a settings file
//! that says nothing cannot switch a frame's passes off.

use crcbl_render::RenderEffects;
use crcbl_store::settings::SettingsStack;

/// The `[engine.video]` section, as a dotted key prefix.
pub const VIDEO_NAMESPACE: &str = "engine.video";

/// Every effect a player can switch off, and the `[engine.video]` key that does
/// it.
///
/// **The one place a key is spelled.** A settings screen writing the row and a
/// start-up reading it back go through this table, because two spellings of one
/// key is a game that saves a setting it will never load again.
///
/// The names are the ones `crcbl_store::settings`' own examples already use for
/// this namespace — bare snake_case nouns beside `vsync` and `master_volume` —
/// rather than the flag spellings (`no_shadows`, `enable_ssao`) the same
/// switches have on a command line. A settings file is a description of what
/// the player wants on, and a negated key would make `shadows = false` and
/// `no_shadows = false` both writable and opposite.
pub const VIDEO_KEYS: [(&str, RenderEffects); 4] = [
    ("shadows", RenderEffects::SHADOWS),
    ("ambient_occlusion", RenderEffects::AMBIENT_OCCLUSION),
    ("reflections", RenderEffects::REFLECTIONS),
    ("bloom", RenderEffects::BLOOM),
];

/// What the player's `[engine.video]` section allows, for
/// [`EffectRequest::video`](crcbl_render::EffectRequest::video).
///
/// [`RenderEffects::all`] for a stack that says nothing, and one bit fewer for
/// each key present and `false`.
///
/// # A line that does nothing says so
///
/// A key holding something that is not a boolean — `shadows = "off"` in a
/// hand-edited file — leaves its effect standing, which is
/// [`SettingsStack::get`]'s own rule for a value it cannot deserialize and the
/// safe direction for a layer that may only remove. It also **warns**, naming
/// the key: silence there is a player who wrote a line, saw no change, and has
/// nothing to read that would tell them why. A key that is simply absent is not
/// a mistake and does not warn.
#[must_use]
pub fn video_effects(stack: &SettingsStack) -> RenderEffects {
    let mut allowed = RenderEffects::all();
    for (key, effect) in VIDEO_KEYS {
        let dotted = format!("{VIDEO_NAMESPACE}.{key}");
        match stack.get::<bool>(&dotted) {
            Some(false) => allowed.remove(effect),
            Some(true) => {}
            // Present, and not something this layer can read. `get` has already
            // searched every layer for a `bool`, so nothing below answered
            // either.
            None if stack.contains(&dotted) => crcbl_core::log::warn!(
                "settings: `{dotted}` is not true or false, so it does nothing; \
                 the effect stays as the game asked for it"
            ),
            None => {}
        }
    }
    allowed
}

#[cfg(test)]
mod tests {
    use super::*;

    use crcbl_store::MemoryStorage;
    use crcbl_store::StorageSource;
    use crcbl_store::settings::SETTINGS_FILE;

    /// A stack over a settings file with `toml` in it, through the real
    /// loader — not a table built in memory, because the spelling of the
    /// section header is half of what this module has to get right.
    fn stack_from(toml: &str) -> SettingsStack {
        let storage = MemoryStorage::new();
        storage
            .write(std::path::Path::new(SETTINGS_FILE), toml.as_bytes())
            .expect("memory storage accepts every write");
        SettingsStack::from_storage(&storage)
    }

    /// **Every effect can be switched off from a settings file.**
    ///
    /// The guard against a bit added to [`RenderEffects`] and not to
    /// [`VIDEO_KEYS`]: the omission has no symptom of its own — the effect
    /// simply cannot be turned off, and a player's row does nothing — so
    /// nothing else would report it.
    #[test]
    fn every_effect_has_a_key_and_no_two_share_one() {
        let mut covered = RenderEffects::empty();
        for (key, effect) in VIDEO_KEYS {
            assert!(
                !covered.intersects(effect),
                "{key} names an effect another key already names"
            );
            covered.insert(effect);
        }
        assert_eq!(
            covered,
            RenderEffects::all(),
            "an effect with no [engine.video] key is one a player cannot switch off"
        );
    }

    /// **A key set to `false` removes exactly its own effect.**
    ///
    /// The pairs are written out rather than taken from [`VIDEO_KEYS`], because
    /// a table used as its own oracle cannot fail: swap two of its rows and a
    /// loop over it still agrees with itself, while every player's settings
    /// file now switches off the wrong effect. These spellings are also the
    /// compatibility promise — renaming one is a file every existing player has
    /// already written.
    #[test]
    fn a_key_set_to_false_removes_that_effect_and_no_other() {
        for (key, effect) in [
            ("shadows", RenderEffects::SHADOWS),
            ("ambient_occlusion", RenderEffects::AMBIENT_OCCLUSION),
            ("reflections", RenderEffects::REFLECTIONS),
            ("bloom", RenderEffects::BLOOM),
        ] {
            let stack = stack_from(&format!("[{VIDEO_NAMESPACE}]\n{key} = false\n"));
            assert_eq!(
                video_effects(&stack),
                RenderEffects::all().difference(effect),
                "{key} = false"
            );
        }
    }

    /// **A key that is absent is not a key that says "off".**
    ///
    /// The arm that fails if a missing key is read as `false`: an empty file,
    /// an `[engine.video]` section naming only one effect, and a key set to
    /// `true` all have to leave every unmentioned effect standing — otherwise
    /// installing the engine turns every effect off for every player who has
    /// never opened a settings screen.
    #[test]
    fn an_absent_key_clamps_nothing() {
        assert_eq!(
            video_effects(&SettingsStack::new()),
            RenderEffects::all(),
            "a stack with no layer at all"
        );
        assert_eq!(
            video_effects(&stack_from("")),
            RenderEffects::all(),
            "a settings file with nothing in it"
        );
        assert_eq!(
            video_effects(&stack_from("[game]\ndifficulty = \"normal\"\n")),
            RenderEffects::all(),
            "a settings file that never mentions video"
        );
        assert_eq!(
            video_effects(&stack_from(&format!("[{VIDEO_NAMESPACE}]\nvsync = true\n"))),
            RenderEffects::all(),
            "an [engine.video] section that names no effect"
        );
        assert_eq!(
            video_effects(&stack_from(&format!(
                "[{VIDEO_NAMESPACE}]\nshadows = false\n"
            ))),
            RenderEffects::all().difference(RenderEffects::SHADOWS),
            "one effect off must leave the two it did not name alone"
        );
    }

    /// **`true` is not "force on", because this layer cannot add.**
    ///
    /// It reads identically to an absent key, which is what makes the layer a
    /// clamp: the row a player set to on is the one the camera stack still
    /// decides.
    #[test]
    fn a_key_set_to_true_reads_the_same_as_no_key_at_all() {
        for (key, _) in VIDEO_KEYS {
            assert_eq!(
                video_effects(&stack_from(&format!("[{VIDEO_NAMESPACE}]\n{key} = true\n"))),
                RenderEffects::all(),
                "{key} = true"
            );
        }
    }

    /// **A value of the wrong type clamps nothing and warns, naming the key.**
    ///
    /// A hand-edited file cannot switch an effect off by accident and cannot
    /// fail the start-up either — and the player who wrote the line hears
    /// about it, because a setting that silently does nothing is
    /// indistinguishable from an engine that ignores the file.
    #[test]
    fn a_key_holding_something_that_is_not_a_boolean_clamps_nothing_and_warns() {
        let capture = crcbl_core::log::capture();
        let stack = stack_from(&format!("[{VIDEO_NAMESPACE}]\nshadows = \"off\"\n"));
        assert_eq!(video_effects(&stack), RenderEffects::all());

        let warned: Vec<_> = capture
            .records()
            .into_iter()
            .filter(|record| record.message.contains("engine.video.shadows"))
            .collect();
        assert_eq!(
            warned.len(),
            1,
            "exactly the key that could not be read: {:?}",
            capture.records()
        );
        assert_eq!(warned[0].level, crcbl_core::log::Level::Warn);
    }

    /// **The keys that read cleanly are silent**, so the warning above stays
    /// worth reading.
    ///
    /// Every arm of the ordinary path: absent, `true`, `false`, and a section
    /// holding a key this layer does not own. A warning that fires for a file
    /// with nothing wrong with it is one a player learns to ignore, and then
    /// the one that matters is ignored too.
    #[test]
    fn a_settings_file_this_layer_can_read_warns_about_nothing() {
        let capture = crcbl_core::log::capture();
        for toml in [
            String::new(),
            format!("[{VIDEO_NAMESPACE}]\nshadows = false\n"),
            format!("[{VIDEO_NAMESPACE}]\nreflections = true\n"),
            format!("[{VIDEO_NAMESPACE}]\nvsync = \"sometimes\"\n"),
        ] {
            let _ = video_effects(&stack_from(&toml));
        }
        let records = capture.records();
        assert!(
            records
                .iter()
                .all(|record| record.level != crcbl_core::log::Level::Warn),
            "nothing here is a mistake this layer can see: {records:?}"
        );
    }

    /// **The highest layer wins, in both directions.**
    ///
    /// `[engine.video]` is a namespace a game may ship defaults for, so the
    /// stack under the user's file is not always empty — and a read that took
    /// the first hit from the bottom, or that unioned the layers, would answer
    /// with a default the player has already overridden. Both directions,
    /// because only one of them fails for either mistake.
    ///
    /// The lower layer is a second file rather than a
    /// [`SettingsLayer::GameDefaults`](crcbl_store::settings::SettingsLayer)
    /// table: naming one would mean this crate depending on the TOML parser it
    /// deliberately reaches only through `crcbl-store`. What is being asserted
    /// is the stack's priority order, which is the same for every layer kind.
    #[test]
    fn the_highest_layer_wins_over_a_default_underneath_it() {
        let mut stack = SettingsStack::new();
        for toml in [
            // The game's defaults: shadows on, reflections off.
            format!("[{VIDEO_NAMESPACE}]\nshadows = true\nreflections = false\n"),
            // The player, disagreeing with both.
            format!("[{VIDEO_NAMESPACE}]\nshadows = false\nreflections = true\n"),
        ] {
            let storage = MemoryStorage::new();
            storage
                .write(std::path::Path::new(SETTINGS_FILE), toml.as_bytes())
                .expect("memory storage accepts every write");
            stack.add(crcbl_store::settings::SettingsLayer::UserFile(
                crcbl_store::settings::StorageSettingsFile::load(
                    &storage,
                    std::path::Path::new(SETTINGS_FILE),
                )
                .expect("a file this test wrote"),
            ));
        }

        assert_eq!(
            video_effects(&stack),
            RenderEffects::all().difference(RenderEffects::SHADOWS),
            "the player's file must beat the layer under it for both keys"
        );
    }
}
