//! The console's variables: the stack a run shares, [`ConsoleHost`], the
//! `save` and `dump` commands, and one [`Binding`] per catalogue key.

use std::any::Any;
use std::cell::{Ref, RefCell, RefMut};
use std::rc::Rc;

use crcbl_audio::mixer::Bus;
use crcbl_console::{Binding, Fault, Flags, Kind, Value};
use crcbl_render::MIN_RENDER_SCALE;
use crcbl_store::settings::SettingsStack;

use super::engine_audio::{AUDIO_NAMESPACE, audio_gains};
use super::engine_display::{
    DISPLAY_MODE_KEY, DISPLAY_MODE_NAMES, PRESENT_MODE_KEY, PRESENT_MODE_NAMES, display_mode,
    display_mode_name, present_mode,
};
use super::engine_video::{
    ANISOTROPIC_FILTERING_KEY, ANTIALIASING_KEY, ANTIALIASING_NAMES, FRAME_LIMIT_CEILING,
    FRAME_LIMIT_KEY, MAX_ANISOTROPIC_FILTERING, MAX_UI_SCALE, MIN_UI_SCALE, RENDER_SCALE_KEY,
    SHADOW_FILTER_KEY, SHADOW_FILTER_NAMES, SSAO_BENT_NORMALS_KEY, SSAO_BENT_NORMALS_KIND,
    SSAO_BLUR_PASSES_KEY, SSAO_BLUR_PASSES_KIND, SSAO_SLICES_KEY, SSAO_SLICES_KIND,
    TIER_VIDEO_KEYS, UI_SCALE_KEY, VIDEO_KEYS, VIDEO_NAMESPACE, anisotropic_filtering,
    antialiasing_or_default, effect_keys, frame_limit, render_scale, shadow_filter,
    ssao_bent_normals, ssao_blur_passes, ssao_slices, ui_scale, video_effects,
};
use super::key_catalogue::{
    ANISOTROPIC_FILTERING_HELP, ANTIALIASING_HELP, DISPLAY_MODE_HELP, EFFECT_HELP,
    FRAME_LIMIT_HELP, GAIN_HELP, GAIN_KIND, NAMED_FLAGS, NAMED_HELP, NAMED_VIDEO_KEYS,
    PRESENT_MODE_HELP, RENDER_SCALE_HELP, SHADOW_FILTER_HELP, SSAO_BENT_NORMALS_HELP,
    SSAO_BLUR_PASSES_HELP, SSAO_SLICES_HELP, UI_SCALE_HELP,
};
use super::stage::{Deferred, apply};

use super::key_catalogue::CatalogueKey;
#[cfg(doc)]
use super::key_catalogue::{KeyStatus, catalogue};

/// One run's settings, held by everything that edits them.
///
/// **A run has one settings file, so it has one stack.** The debug console and
/// a game's own settings screen both read keys and write them back, and a copy
/// each is two writers over one path: a key set on the screen is not what the
/// console prints, and whichever of them saves last wins. This is the handle
/// they share — [`HostedGame::settings`](crate::engine::HostedGame::settings)
/// is where a game hands it to the loop, and [`ConsoleHost`] is what the loop
/// puts it in.
///
/// # Why shared ownership rather than a borrow
///
/// A [`Binding`] reaches its host as `&mut dyn Any`, and [`Any`] is implemented
/// only for `'static` types — so a [`ConsoleHost`] cannot hold a borrow of
/// anything the loop owns, which is the same constraint [`Deferred`] exists
/// for. [`Rc`] is what is left, and a [`RefCell`] inside it because both
/// holders write. Nothing here crosses a thread: a loop and its game are one
/// thread's, natively and in a browser alike.
///
/// # Borrows are for the length of one read or one write
///
/// [`stack_mut`](Self::stack_mut) panics while another borrow is live, on
/// [`RefCell`]'s own terms, and the other holder is reachable from any call
/// that leaves this crate. So a borrow is taken, used and dropped without a
/// call to the console or the game in between; one held across such a call is
/// what makes that call panic, and nothing but this rule prevents it.
#[derive(Clone, Debug, Default)]
pub struct SharedSettings(Rc<RefCell<SettingsStack>>);

impl SharedSettings {
    /// A handle over `stack`, with no other holder yet.
    #[must_use]
    pub fn new(stack: SettingsStack) -> Self {
        Self(Rc::new(RefCell::new(stack)))
    }

    /// The settings as they stand.
    ///
    /// # Panics
    ///
    /// If a [`stack_mut`](Self::stack_mut) borrow is still live.
    #[must_use]
    pub fn stack(&self) -> Ref<'_, SettingsStack> {
        self.0.borrow()
    }

    /// The settings, to write.
    ///
    /// `&self` rather than `&mut self` because that is the whole point of the
    /// type: the other holder is editing the same stack through a handle of its
    /// own.
    ///
    /// # Panics
    ///
    /// If any other borrow is still live.
    #[must_use]
    pub fn stack_mut(&self) -> RefMut<'_, SettingsStack> {
        self.0.borrow_mut()
    }
}

/// The state a settings console variable reads and writes: the player's stack,
/// and what a write still owes the process.
///
/// **The type every [`console_bindings`] binding downcasts `&mut dyn Any` to**,
/// and the reason it owns its two halves rather than borrowing them:
/// [`Any`] is implemented only for `'static` types, so a host cannot hold the
/// renderer or the mixer a write has to reach. It holds the stack — through a
/// [`SharedSettings`], so the game's settings screen is editing the same one —
/// and a [`Deferred`] — see that type. `Loop::new` builds one and the frame
/// drains it; that is debug-console slice 5, recorded in
/// `docs/notes/tooling.md`.
#[derive(Debug, Default)]
pub struct ConsoleHost {
    pub(super) stack: SharedSettings,
    pub(super) pending: Deferred,
    engine: crate::debug_console::EngineLink,
}

impl ConsoleHost {
    /// A host over `stack` alone, with nothing pending and nowhere to save.
    ///
    /// What a run whose game keeps no settings of its own gets — every sample
    /// but `apps/options`. [`over`](Self::over) is the other half.
    #[must_use]
    pub fn new(stack: SettingsStack) -> Self {
        Self::over(SharedSettings::new(stack))
    }

    /// A host over a stack the game is editing too.
    ///
    /// What `Loop::new` builds from
    /// [`HostedGame::settings`](crate::engine::HostedGame::settings), so the
    /// console's `music_volume` and a settings screen's fader are one key in
    /// one file rather than two copies of it.
    #[must_use]
    pub fn over(stack: SharedSettings) -> Self {
        Self {
            stack,
            pending: Deferred::new(),
            engine: crate::debug_console::EngineLink::new(),
        }
    }

    /// This host, with `save` writing the platform settings file for
    /// `app_name`.
    ///
    /// Left unset by [`new`](Self::new) rather than defaulted to the game's
    /// name, because the run that must not write one is exactly the run that
    /// would not notice: a golden comparison or a determinism harness takes its
    /// settings from [`SettingsSource::None`](crate::engine::SettingsSource) and
    /// must not persist into whichever home directory it executes in. The
    /// caller that read a file is the caller that names it.
    #[must_use]
    pub fn saving_as(mut self, app_name: &str) -> Self {
        self.engine.app_name = Some(app_name.to_owned());
        self
    }

    /// The settings as they stand.
    #[must_use]
    pub fn stack(&self) -> Ref<'_, SettingsStack> {
        self.stack.stack()
    }

    /// The settings, to write — what `save` and `dump` reach.
    #[must_use]
    pub fn stack_mut(&mut self) -> RefMut<'_, SettingsStack> {
        self.stack.stack_mut()
    }

    /// What a write has asked for and nothing has applied yet.
    pub const fn pending_mut(&mut self) -> &mut Deferred {
        &mut self.pending
    }

    /// What the console's commands have asked of the loop, to read.
    #[must_use]
    pub const fn engine(&self) -> &crate::debug_console::EngineLink {
        &self.engine
    }

    /// What the console's commands have asked of the loop, to record and to
    /// drain.
    pub const fn engine_mut(&mut self) -> &mut crate::debug_console::EngineLink {
        &mut self.engine
    }
}

crcbl_console::concommand! {
    /// Write the settings file. Nothing a console sets is saved until this runs.
    pub fn save(cx, _args) {
        let saved = {
            let host = cx
                .host_mut()
                .downcast_mut::<ConsoleHost>()
                .expect("the engine's console is only ever run over a `ConsoleHost`");
            let Some(app_name) = host.engine.app_name.clone() else {
                // Deliberately a fault rather than a quiet success: a run with
                // no file to write is the golden-run case, and "saved" arriving
                // for a write that went nowhere is the failure this module's
                // `Unsupported` exists to refuse.
                return Err(Fault::new(
                    "this run reads no settings file, so there is nowhere to save to",
                ));
            };
            host.stack
                .stack()
                .save_platform(&app_name)
                .map_err(|error| Fault::new(error.to_string()))?;
            app_name
        };
        cx.print(format!("settings saved for `{saved}`"));
        Ok(())
    }
}

crcbl_console::concommand! {
    /// Print every key the settings stack holds, and the layer it came from.
    pub fn dump(cx, _args) {
        let lines = dump_lines(
            &cx.host()
                .downcast_ref::<ConsoleHost>()
                .expect("the engine's console is only ever run over a `ConsoleHost`")
                .stack
                .stack(),
        );
        for line in lines {
            cx.print(line);
        }
        Ok(())
    }
}

/// `dump`'s lines: `key = value  (layer)`, one per key the stack holds.
///
/// The layer is the point — a value nobody remembers choosing is traced to the
/// game's defaults, the player's file or this run's `--set` — and it is
/// [`SettingsStack::entries`]' word for it, the one `crcbl settings list`
/// prints too.
fn dump_lines(stack: &SettingsStack) -> Vec<String> {
    stack
        .entries()
        .into_iter()
        .map(|entry| format!("{} = {}  ({})", entry.key, entry.value, entry.layer.name()))
        .collect()
}

/// Every catalogue key as a console variable, in [`catalogue`]'s order.
///
/// One [`Binding`] per key: the key's bare name as the console name
/// ([`CatalogueKey::name`]), the catalogue's [`Kind`] and help, [`Flags::ARCHIVE`]
/// because the settings stack is where the value lives, and
/// [`Flags::READ_ONLY`] beside it for a [`KeyStatus::Named`] key so the console
/// prints the whole catalogue and refuses to write the half nothing reads.
///
/// # Why a macro over a static list, and not one generic pair of functions
///
/// A [`Binding`]'s `get` and `set` are bare `fn` pointers and are **not** handed
/// the binding they belong to, so a single pair could not know which key it was
/// called for; the name has to be baked into the function. The macro bakes it,
/// one tiny pair per key, each forwarding to the one `read`/`write` body below —
/// so there is one copy of the logic and N copies of a name, which is the
/// direction that cannot drift. `the_bindings_are_the_catalogue` holds the list
/// to [`catalogue`] itself, so a key added to one and forgotten in the other is
/// a red test rather than a variable the console does not have.
#[must_use]
pub const fn console_bindings() -> &'static [&'static Binding] {
    BINDINGS
}

/// Read `namespace.name` off a [`ConsoleHost`], as `kind`'s [`Value`].
///
/// Through the readers rather than off the stack directly, so what the console
/// prints is what the engine would read — including the clamp, the default for
/// an absent key and the warning for a line that says nothing.
fn read(host: &dyn Any, namespace: &str, name: &str, kind: Kind) -> Value {
    let stack = &host
        .downcast_ref::<ConsoleHost>()
        .expect("a settings binding is only ever given a `ConsoleHost`")
        .stack
        .stack();
    read_stack(stack, namespace, name, kind)
}

/// What the engine reads for `entry` out of `stack`: the value the console
/// prints for its variable, and what `crcbl settings list` prints for a key
/// no layer holds — the engine's own default, through the same reader.
#[must_use]
pub fn catalogue_value(stack: &SettingsStack, entry: &CatalogueKey) -> Value {
    let namespace = entry
        .key
        .strip_suffix(entry.name)
        .and_then(|prefix| prefix.strip_suffix('.'))
        .expect("a catalogue key is its namespace and its name, joined by a dot");
    read_stack(stack, namespace, entry.name, entry.kind)
}

/// [`read`], on the stack itself.
fn read_stack(stack: &SettingsStack, namespace: &str, name: &str, kind: Kind) -> Value {
    if namespace == AUDIO_NAMESPACE {
        let gains = audio_gains(stack);
        let (_, gain) = gains
            .into_iter()
            .find(|(bus, _)| bus.settings_key() == name)
            .expect("every audio binding names a bus");
        return Value::Float(gain);
    }
    match name {
        FRAME_LIMIT_KEY => Value::Int(i64::from(frame_limit(stack).rate())),
        // There is no word for "the player has not picked one" — see
        // `set_video`'s docs — so an absent key reads back as the rung it leaves
        // the game on, which is what `apps/options`' row shows for it too.
        ANTIALIASING_KEY => Value::Enum(antialiasing_or_default(stack).name()),
        SHADOW_FILTER_KEY => Value::Enum(shadow_filter(stack).label()),
        SSAO_SLICES_KEY => Value::Int(ssao_slices(stack)),
        SSAO_BLUR_PASSES_KEY => Value::Int(ssao_blur_passes(stack)),
        SSAO_BENT_NORMALS_KEY => Value::Bool(ssao_bent_normals(stack)),
        RENDER_SCALE_KEY => Value::Float(render_scale(stack)),
        ANISOTROPIC_FILTERING_KEY => Value::Float(anisotropic_filtering(stack)),
        UI_SCALE_KEY => Value::Float(ui_scale(stack)),
        // An absent key reads back as the word for what a run opens on when
        // the player has said nothing — windowed, and `auto` pacing — which is
        // the game's own choice in every sample here.
        DISPLAY_MODE_KEY => Value::Enum(display_mode_name(display_mode(stack).unwrap_or_default())),
        PRESENT_MODE_KEY => Value::Enum(present_mode(stack).unwrap_or_default().name()),
        _ => match effect_keys().find(|(candidate, _)| *candidate == name) {
            Some((_, effect)) => Value::Bool(video_effects(stack).contains(effect)),
            // A `Named` key: nothing reads it, so there is nothing to read it
            // back through. Its binding is `READ_ONLY`, and this is what `help`
            // prints beside it.
            None => named_value(stack, namespace, name, kind),
        },
    }
}

/// What a [`KeyStatus::Named`] key reads back as: whatever the file holds,
/// coerced through the kind the catalogue declared for it.
///
/// Straight off the stack rather than through a reader, because the whole of
/// what makes the key `Named` is that it has no reader. A key the file does not
/// hold — the ordinary case — reads back as the kind's own floor, which is what
/// `help` prints beside "nothing reads this yet".
fn named_value(stack: &SettingsStack, namespace: &str, name: &str, kind: Kind) -> Value {
    let dotted = format!("{namespace}.{name}");
    match kind {
        Kind::Bool => Value::Bool(stack.get::<bool>(&dotted).unwrap_or_default()),
        Kind::Int { min, .. } => Value::Int(stack.get::<i64>(&dotted).unwrap_or(min)),
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a settings float is read as f64 and shown as f32, which is the width every other reader here answers in"
        )]
        // Not clamped to the kind: the file holds what a hand-edit put there,
        // and a row that reported the floor instead would be this reader
        // inventing the value it was asked to show. Nothing can be set through
        // it either way — the binding is `READ_ONLY`.
        Kind::Float { min, .. } => {
            Value::Float(stack.get::<f64>(&dotted).map_or(min, |value| value as f32))
        }
        Kind::Enum(values) => Value::Enum(
            stack
                .get::<String>(&dotted)
                .and_then(|held| {
                    values
                        .iter()
                        .copied()
                        .find(|candidate| *candidate == held.as_str())
                })
                .unwrap_or(values[0]),
        ),
        Kind::Text => Value::Text(stack.get::<String>(&dotted).unwrap_or_default()),
    }
}

/// Write `namespace.name` through [`apply`], on the host's own [`Deferred`]
/// stage.
///
/// # Errors
///
/// [`apply`]'s.
fn write(host: &mut dyn Any, namespace: &str, name: &str, value: &Value) -> Result<(), Fault> {
    let host = host
        .downcast_mut::<ConsoleHost>()
        .expect("a settings binding is only ever given a `ConsoleHost`");
    let mut stack = host.stack.stack_mut();
    apply(
        &mut stack,
        &format!("{namespace}.{name}"),
        value,
        &mut host.pending,
    )
    .map(|_| ())
}

/// One [`Binding`] per catalogue key, each with its own name baked in.
macro_rules! settings_bindings {
    ($(
        $binding:ident: $namespace:expr, $name:expr, $kind:expr, $flags:expr, $help:expr;
    )*) => {
        $(
            static $binding: Binding = {
                fn get(host: &dyn Any) -> Value {
                    read(host, $namespace, $name, $kind)
                }
                fn set(host: &mut dyn Any, value: &Value) -> Result<(), Fault> {
                    write(host, $namespace, $name, value)
                }
                Binding::new($name, $help, $kind, $flags, get, set)
            };
        )*

        /// The list [`console_bindings`] answers with.
        static BINDINGS: &[&Binding] = &[$(&$binding),*];
    };
}

settings_bindings! {
    SHADOWS: VIDEO_NAMESPACE, VIDEO_KEYS[0].0, Kind::Bool, Flags::ARCHIVE, EFFECT_HELP;
    AMBIENT_OCCLUSION: VIDEO_NAMESPACE, VIDEO_KEYS[1].0, Kind::Bool, Flags::ARCHIVE, EFFECT_HELP;
    REFLECTIONS: VIDEO_NAMESPACE, VIDEO_KEYS[2].0, Kind::Bool, Flags::ARCHIVE, EFFECT_HELP;
    BLOOM: VIDEO_NAMESPACE, VIDEO_KEYS[3].0, Kind::Bool, Flags::ARCHIVE, EFFECT_HELP;
    VOLUMETRIC_FOG: VIDEO_NAMESPACE, VIDEO_KEYS[4].0, Kind::Bool, Flags::ARCHIVE, EFFECT_HELP;
    AUTO_EXPOSURE: VIDEO_NAMESPACE, VIDEO_KEYS[5].0, Kind::Bool, Flags::ARCHIVE, EFFECT_HELP;
    CONTACT_SHADOWS: VIDEO_NAMESPACE, TIER_VIDEO_KEYS[0].0, Kind::Bool, Flags::ARCHIVE, EFFECT_HELP;

    ANTIALIASING: VIDEO_NAMESPACE, ANTIALIASING_KEY, Kind::Enum(&ANTIALIASING_NAMES),
        Flags::ARCHIVE, ANTIALIASING_HELP;
    SHADOW_FILTER: VIDEO_NAMESPACE, SHADOW_FILTER_KEY, Kind::Enum(&SHADOW_FILTER_NAMES),
        Flags::ARCHIVE, SHADOW_FILTER_HELP;
    SSAO_SLICES: VIDEO_NAMESPACE, SSAO_SLICES_KEY, SSAO_SLICES_KIND,
        Flags::ARCHIVE, SSAO_SLICES_HELP;
    SSAO_BLUR_PASSES: VIDEO_NAMESPACE, SSAO_BLUR_PASSES_KEY, SSAO_BLUR_PASSES_KIND,
        Flags::ARCHIVE, SSAO_BLUR_PASSES_HELP;
    SSAO_BENT_NORMALS: VIDEO_NAMESPACE, SSAO_BENT_NORMALS_KEY, SSAO_BENT_NORMALS_KIND,
        Flags::ARCHIVE, SSAO_BENT_NORMALS_HELP;
    RENDER_SCALE: VIDEO_NAMESPACE, RENDER_SCALE_KEY,
        Kind::Float { min: MIN_RENDER_SCALE, max: 1.0 }, Flags::ARCHIVE, RENDER_SCALE_HELP;
    ANISOTROPIC_FILTERING: VIDEO_NAMESPACE, ANISOTROPIC_FILTERING_KEY,
        Kind::Float { min: 1.0, max: MAX_ANISOTROPIC_FILTERING }, Flags::ARCHIVE,
        ANISOTROPIC_FILTERING_HELP;
    FRAME_LIMIT: VIDEO_NAMESPACE, FRAME_LIMIT_KEY,
        Kind::Int { min: 0, max: FRAME_LIMIT_CEILING }, Flags::ARCHIVE, FRAME_LIMIT_HELP;
    UI_SCALE: VIDEO_NAMESPACE, UI_SCALE_KEY,
        Kind::Float { min: MIN_UI_SCALE, max: MAX_UI_SCALE }, Flags::ARCHIVE, UI_SCALE_HELP;

    // Written outright from here, never held for a confirm: the console is a
    // developer's, and `apply` answers it "next start" because a console host
    // has no window or swapchain to put either into force on.
    DISPLAY_MODE: VIDEO_NAMESPACE, DISPLAY_MODE_KEY, Kind::Enum(&DISPLAY_MODE_NAMES),
        Flags::ARCHIVE, DISPLAY_MODE_HELP;
    PRESENT_MODE: VIDEO_NAMESPACE, PRESENT_MODE_KEY, Kind::Enum(&PRESENT_MODE_NAMES),
        Flags::ARCHIVE, PRESENT_MODE_HELP;

    MONITOR: VIDEO_NAMESPACE, NAMED_VIDEO_KEYS[0].0, NAMED_VIDEO_KEYS[0].1,
        NAMED_FLAGS, NAMED_HELP[0];
    RESOLUTION: VIDEO_NAMESPACE, NAMED_VIDEO_KEYS[1].0, NAMED_VIDEO_KEYS[1].1,
        NAMED_FLAGS, NAMED_HELP[1];
    BRIGHTNESS: VIDEO_NAMESPACE, NAMED_VIDEO_KEYS[2].0, NAMED_VIDEO_KEYS[2].1,
        NAMED_FLAGS, NAMED_HELP[2];
    HDR_OUTPUT: VIDEO_NAMESPACE, NAMED_VIDEO_KEYS[3].0, NAMED_VIDEO_KEYS[3].1,
        NAMED_FLAGS, NAMED_HELP[3];
    FOV: VIDEO_NAMESPACE, NAMED_VIDEO_KEYS[4].0, NAMED_VIDEO_KEYS[4].1,
        NAMED_FLAGS, NAMED_HELP[4];

    MASTER_VOLUME: AUDIO_NAMESPACE, Bus::ALL[0].settings_key(), GAIN_KIND,
        Flags::ARCHIVE, GAIN_HELP;
    MUSIC_VOLUME: AUDIO_NAMESPACE, Bus::ALL[1].settings_key(), GAIN_KIND,
        Flags::ARCHIVE, GAIN_HELP;
    SFX_VOLUME: AUDIO_NAMESPACE, Bus::ALL[2].settings_key(), GAIN_KIND,
        Flags::ARCHIVE, GAIN_HELP;
    UI_VOLUME: AUDIO_NAMESPACE, Bus::ALL[3].settings_key(), GAIN_KIND,
        Flags::ARCHIVE, GAIN_HELP;
    VOICE_VOLUME: AUDIO_NAMESPACE, Bus::ALL[4].settings_key(), GAIN_KIND,
        Flags::ARCHIVE, GAIN_HELP;
    AMBIENCE_VOLUME: AUDIO_NAMESPACE, Bus::ALL[5].settings_key(), GAIN_KIND,
        Flags::ARCHIVE, GAIN_HELP;
}

#[cfg(test)]
mod tests {
    use super::*;

    use crcbl_render::shadow::Filter;

    use crate::settings::tests::{Recorder, round_trip, stack_from};
    use crate::settings::{Applied, KeyStatus, Stage, catalogue, set_ui_scale};

    /// **The UI multiplier is stored clamped, refuses a non-number, and a write
    /// through [`apply`] lands in the file and reaches the host that draws the
    /// UI** — live there, and next start on a host that draws none.
    #[test]
    fn a_ui_scale_is_written_clamped_and_reaches_the_host_that_draws_it() {
        let (reloaded, written) = round_trip(|stack| {
            set_ui_scale(stack, 10.0).expect("a fresh user layer accepts every key");
        });
        assert_eq!(ui_scale(&reloaded), MAX_UI_SCALE);
        assert!(!written.contains("10"), "the unclamped ask:\n{written}");

        let mut stack = stack_from("");
        assert!(set_ui_scale(&mut stack, f32::NAN).is_err());
        let key = format!("{VIDEO_NAMESPACE}.{UI_SCALE_KEY}");
        assert!(!stack.contains(&key), "a refused write reached the stack");

        let mut stage = Recorder::default();
        assert_eq!(
            apply(&mut stack, &key, &Value::Float(1.5), &mut stage),
            Ok(Applied::Live)
        );
        assert_eq!(ui_scale(&stack), 1.5);
        assert_eq!(
            stage.ui_scales,
            [1.5],
            "the host drawing the UI was not told"
        );
        assert!(stage.video.is_empty(), "the renderer was told about the UI");

        struct Nowhere;
        impl Stage for Nowhere {}
        assert_eq!(
            apply(&mut stack, &key, &Value::Float(2.0), &mut Nowhere),
            Ok(Applied::NextStart)
        );
        assert_eq!(ui_scale(&stack), 2.0, "a host with no seam still writes");

        // The console's host records it for the loop to drain.
        let mut host = ConsoleHost::new(stack_from(""));
        binding_for(UI_SCALE_KEY)
            .set(&mut host, &Value::Float(1.25))
            .expect("inside the range");
        assert_eq!(host.pending_mut().take_ui_scale(), Some(1.25));
        assert!(
            host.pending_mut().is_empty(),
            "the drain left the ask behind"
        );
    }

    /// The values a kind's own ends are, for a sweep that has to touch both.
    fn ends_of(kind: Kind) -> Vec<Value> {
        match kind {
            Kind::Bool => vec![Value::Bool(false), Value::Bool(true)],
            Kind::Int { min, max } => vec![Value::Int(min), Value::Int(max)],
            Kind::Float { min, max } => vec![Value::Float(min), Value::Float(max)],
            Kind::Enum(values) => values.iter().copied().map(Value::Enum).collect(),
            Kind::Text => vec![Value::Text("anything".to_owned())],
        }
    }

    /// The binding the console reaches `name` through.
    fn binding_for(name: &str) -> &'static Binding {
        console_bindings()
            .iter()
            .copied()
            .find(|binding| binding.name() == name)
            .unwrap_or_else(|| panic!("`{name}` has no console binding"))
    }

    /// **Every numeric kind's own range is the range its setter stores**, at
    /// both ends.
    ///
    /// The failure this exists for is the one debug-console decision 3
    /// (`docs/notes/tooling.md`) names: a domain that says "1 to 16" while the
    /// setter clamps to something else, so a console accepts a value the file
    /// then reads back as a different one. Written as a sweep over
    /// [`catalogue`] rather than a list, so a key added with a hand-written
    /// range joins it the day it lands — and the count is asserted, because a
    /// sweep that matched nothing would pass in silence.
    #[test]
    fn every_kind_admits_the_ends_of_its_own_range_and_reads_them_back() {
        let mut checked = 0;
        for entry in catalogue() {
            if entry.status != KeyStatus::Read {
                continue;
            }
            for end in ends_of(entry.kind) {
                let binding = binding_for(entry.name);
                let mut host = ConsoleHost::new(stack_from(""));
                binding
                    .set(&mut host, &end)
                    .unwrap_or_else(|fault| panic!("`{}` refused {end}: {fault}", entry.key));
                assert_eq!(
                    binding.get(&host),
                    end,
                    "`{}` did not read back the value its own kind admits",
                    entry.key,
                );
                assert!(
                    host.stack().contains(&entry.key),
                    "`{}` was applied without being written",
                    entry.key,
                );
                checked += 1;
            }
        }
        // Seven switches, eleven video rows and six gains: two ends of each
        // numeric or boolean row, and every word of each enum row.
        assert_eq!(checked, 52, "the sweep did not cover the read catalogue");
    }

    /// **A key nothing reads refuses a set, and says why.**
    ///
    /// The console half of [`KeyStatus::Named`]: a control that silently does
    /// nothing is worse than one that says so, which is what that enum exists
    /// for — and the binding carries [`Flags::READ_ONLY`] so the refusal
    /// happens before [`apply`] is even reached.
    #[test]
    fn a_key_nothing_reads_refuses_a_set_through_both_doors() {
        let key = format!("{VIDEO_NAMESPACE}.fov");
        let mut stack = stack_from("");
        let mut stage = Recorder::default();
        let fault = apply(&mut stack, &key, &Value::Float(90.0), &mut stage)
            .expect_err("nothing reads the field of view");
        assert!(
            fault.message().contains("nothing reads this yet"),
            "{}",
            fault.message()
        );
        assert!(!stack.contains(&key), "a refused write reached the file");

        let binding = binding_for("fov");
        assert!(binding.flags().contains(Flags::READ_ONLY));
        let mut host = ConsoleHost::new(stack_from(""));
        assert_eq!(
            binding
                .set(&mut host, &Value::Float(90.0))
                .expect_err("read only")
                .message(),
            "`fov` is read-only"
        );
    }

    /// **A write the console makes is waiting for the frame that can show it.**
    ///
    /// The console's host cannot hold the renderer — see [`Deferred`] — so the
    /// claim that has to hold is that the ask survives to be drained, and that
    /// the drain empties it.
    #[test]
    fn a_console_write_is_recorded_for_the_frame_to_drain() {
        let binding = binding_for(RENDER_SCALE_KEY);
        let mut host = ConsoleHost::new(stack_from(""));
        assert!(host.pending_mut().is_empty());

        binding
            .set(&mut host, &Value::Float(0.5))
            .expect("inside the range");
        let taken = host.pending_mut().take_video().expect("the frame has work");
        assert!((taken.render_scale - 0.5).abs() < f32::EPSILON);
        binding_for(SHADOW_FILTER_KEY)
            .set(&mut host, &Value::Enum(Filter::Box.label()))
            .expect("the filter is in the catalogue domain");
        assert_eq!(
            host.pending_mut()
                .take_video()
                .expect("the frame has the filter write")
                .shadow_filter,
            Filter::Box
        );
        assert!(
            host.pending_mut().is_empty(),
            "the drain left the ask behind, so the next frame would apply it again"
        );
    }

    /// **There is one console variable per catalogue key, under the key's own
    /// bare name.**
    ///
    /// Both directions, because either alone passes on an empty list: the count
    /// against [`catalogue`], and every binding's name back to a catalogue
    /// entry with the same kind and help.
    #[test]
    fn the_bindings_are_the_catalogue() {
        let catalogue = catalogue();
        assert_eq!(
            console_bindings().len(),
            catalogue.len(),
            "the macro's list and the catalogue disagree about how many keys there are",
        );
        for binding in console_bindings() {
            let entry = catalogue
                .iter()
                .find(|entry| entry.name == binding.name())
                .unwrap_or_else(|| panic!("`{}` is a variable and not a key", binding.name()));
            assert_eq!(binding.kind(), entry.kind, "`{}`", entry.key);
            assert_eq!(binding.help(), entry.help, "`{}`", entry.key);
            assert!(
                binding.flags().contains(Flags::ARCHIVE),
                "`{}` is not persisted, though the settings stack is its storage",
                entry.key,
            );
            assert_eq!(
                binding.flags().contains(Flags::READ_ONLY),
                entry.status == KeyStatus::Named,
                "`{}`'s console flag disagrees with its catalogue status",
                entry.key,
            );
        }
    }

    /// **A `READ_ONLY` binding still prints**, which is the point of listing the
    /// half of the catalogue nothing reads.
    #[test]
    fn a_key_nothing_reads_still_prints_what_the_file_holds() {
        let host = ConsoleHost::new(stack_from("[engine.video]\nfov = 75.0\n"));
        assert_eq!(binding_for("fov").get(&host), Value::Float(75.0));
        assert_eq!(
            binding_for("display_mode").get(&host),
            Value::Enum("windowed"),
            "an absent enum reads back as the first name in its set"
        );
    }

    /// **`dump` names the layer each key came from**: the game's default, the
    /// player's file and this run's `--set`, each on its own line.
    #[test]
    fn dump_names_the_layer_each_key_came_from() {
        let mut launch = crcbl_store::settings::LaunchLayers::new()
            .with_game_defaults(
                "[game]
lives = 2
speed = 1",
            )
            .expect("a test's own TOML");
        launch.set("game.lives=4").expect("a well-formed override");
        let storage = crcbl_store::MemoryStorage::new();
        crcbl_store::StorageSource::write(
            &storage,
            std::path::Path::new(crcbl_store::settings::SETTINGS_FILE),
            b"[game]
name = \"Ada\"
",
        )
        .expect("memory storage accepts every write");
        let stack = SettingsStack::from_storage_with(&storage, &launch);

        assert_eq!(
            dump_lines(&stack),
            [
                "game.lives = 4  (cli)",
                "game.name = \"Ada\"  (user)",
                "game.speed = 1  (game)",
            ]
        );
    }

    /// **The value a key no layer holds is the engine's own reading**, the one
    /// its console variable prints.
    #[test]
    fn a_catalogue_value_is_what_the_engines_reader_answers() {
        let render_scale = catalogue()
            .into_iter()
            .find(|entry| entry.name == RENDER_SCALE_KEY)
            .expect("the render scale is catalogued");
        assert_eq!(
            catalogue_value(&stack_from(""), &render_scale),
            Value::Float(1.0)
        );
        assert_eq!(
            catalogue_value(
                &stack_from(
                    "[engine.video]
render_scale = 0.5"
                ),
                &render_scale
            ),
            Value::Float(0.5)
        );
    }
}
