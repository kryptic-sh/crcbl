//! What a failed `MTLCommandBuffer` is made to say, and the descriptor that
//! makes it able to say it.
//!
//! # A GPU fault that names no encoder is the failure mode this crate keeps
//! closing
//!
//! When the GPU faults, Metal fails the command buffer and hands back an
//! `NSError` whose text is one sentence — `Caused GPU Hang Error
//! (00000003:kIOGPUCommandBufferCallbackErrorHang)` and nothing else. A command
//! buffer holds several encoders, so that sentence says the submission died
//! without saying *where*, and the difference between "the render pass faulted"
//! and "the blit after it never started" is the whole of the investigation.
//! `crcbl_mtl::device`'s wait-before-signal refusal exists for the same class of
//! failure: the process is alive, the work is gone, and nothing is in any log.
//!
//! Metal's own answer is
//! [`MTLCommandBufferErrorOption::EncoderExecutionStatus`], set on the
//! `MTLCommandBufferDescriptor` a command buffer is created from. With it, a
//! failed command buffer's `NSError` carries an
//! `MTLCommandBufferEncoderInfoErrorKey` entry: one
//! [`MTLCommandBufferEncoderInfo`] per encoder, **in recorded order**, each with
//! the encoder's label, the debug signposts inserted into it, and an
//! [`MTLCommandEncoderErrorState`] — `Faulted` for the encoder that caused the
//! error, `Affected` for one caught up in it, `Pending` for one that never
//! started, `Completed` for one that finished.
//!
//! So [`command_buffer`] is the only way this backend creates one, and
//! [`describe`] is the only way it reads a failure back. Apple notes the option
//! "may increase CPU, GPU, and/or memory overhead on some platforms"; it is
//! taken unconditionally anyway, because a backend that can only be debugged
//! after someone rebuilds it with a flag is one nobody debugs from a CI log —
//! which is the only place a GPU fault on another machine is ever seen.
//!
//! **Every encoder is labelled**, and that is part of this and not decoration:
//! an encoder with no label reports as an empty string here, which turns the
//! whole diagnostic back into "one of them faulted".
//!
//! # Metal's validation is not Vulkan's, and this module says so rather than
//! pretending otherwise
//!
//! `crcbl-vk` gets every validation message through a callback into a sink it
//! owns, counts them, and fails a test on a non-zero count.
//! `crcbl_dx12::debug` does the same through an `ID3D12InfoQueue`. **Metal
//! offers neither.** What it offers is three separate things, and only the last
//! is data this process can read:
//!
//! * **API validation** — [`DEBUG_LAYER_ENV_VAR`], read by Metal itself when
//!   the framework loads, never by this crate. A misuse is *printed* and then
//!   handled per [`ERROR_MODE_ENV_VAR`] / [`WARNING_MODE_ENV_VAR`]: ignored,
//!   asserted, aborted, or logged. There is no list to query and no callback to
//!   install, so the strongest form the check can take is **the process did not
//!   die** — which is asserted by the test runner reaping a killed process, not
//!   by any assertion in this crate.
//! * **Shader validation** — [`SHADER_VALIDATION_ENV_VAR`], GPU-side bounds and
//!   access checking. Its findings surface as a *failed command buffer*, which
//!   is the one queryable channel.
//! * **Execution faults** — a page fault, a hang, a timeout. Also a failed
//!   command buffer, and the thing the rest of this module was written for.
//!
//! So the honest Metal equivalent of `assert_clean` is
//! [`ValidationReport::assert_clean`], and it asserts two things:
//!
//! 1. the API validation layer really was interposed on this device, and
//! 2. no command buffer this device submitted ended in
//!    [`MTLCommandBufferStatus::Error`].
//!
//! **That is weaker than Vulkan's and D3D12's, and the gap is worth naming.** A
//! Metal API misuse does not reach (2) at all — it is a message on stderr and,
//! at the default error mode, a dead process. There is no count of validation
//! messages, so "zero errors" cannot be asserted the way it is for the other two
//! backends; what can be asserted is "the checking was switched on, and nothing
//! it checks reported back".
//!
//! ## How "the layer was interposed" is answered, and why it is a private detail
//!
//! Metal publishes no API for it. What it does do is **wrap** the `MTLDevice` in
//! a validating subclass when [`DEBUG_LAYER_ENV_VAR`] is set, so the object's
//! Objective-C class changes from something like `MTLIGAccelDevice` to
//! `MTLDebugDevice`. [`layer_wrapped_device`] reads that name. It is an
//! implementation detail of Metal and could be renamed by any macOS release —
//! but the alternative is asserting on the environment variable this process
//! exported, which proves only that a variable was set and is exactly the
//! "check that cannot fail" this whole exercise is about. A rename fails loudly
//! and names the class it saw, which is a diagnosable failure; the variable
//! would silently keep passing.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, AnyProtocol, ProtocolObject};
use objc2_foundation::{NSArray, NSError, NSString};
use objc2_metal::{
    MTLCommandBuffer, MTLCommandBufferDescriptor, MTLCommandBufferEncoderInfo,
    MTLCommandBufferEncoderInfoErrorKey, MTLCommandBufferErrorOption, MTLCommandBufferStatus,
    MTLCommandEncoderErrorState, MTLCommandQueue, MTLDevice,
};

/// Metal's API-validation switch. **Read here, never set here** — Metal reads
/// it when the framework loads, which is before anything in this crate runs, so
/// setting it from inside the process would be too late as well as unsound in a
/// test binary with threads. `crates/crcbl-mtl/tests/run-mtl-e2e.sh` exports it.
pub(crate) const DEBUG_LAYER_ENV_VAR: &str = "MTL_DEBUG_LAYER";

/// What the API-validation layer does with an error: `ignore`, `assert`,
/// `abort` or `nslog`. Reported so a green run states what a violation would
/// have done, rather than leaving it to whatever the platform defaults to.
pub(crate) const ERROR_MODE_ENV_VAR: &str = "MTL_DEBUG_LAYER_ERROR_MODE";

/// As [`ERROR_MODE_ENV_VAR`], for warnings. `crcbl-vk`'s line is zero errors
/// *and* zero warnings, so the harness sets this to the same thing it sets the
/// error mode to.
pub(crate) const WARNING_MODE_ENV_VAR: &str = "MTL_DEBUG_LAYER_WARNING_MODE";

/// GPU-side shader validation — bounds and access checking inside a running
/// kernel. Unlike API validation, what it finds arrives as a **failed command
/// buffer**, which is the one validation channel this process can read.
pub(crate) const SHADER_VALIDATION_ENV_VAR: &str = "MTL_SHADER_VALIDATION";

/// How many command-buffer failures are kept verbatim.
///
/// A cap rather than the whole history: a device that has gone wrong fails every
/// submission after the first, and an assertion carrying hundreds of copies of
/// one hang is one nobody reads. The count stays exact past it.
const MAX_KEPT_FAULTS: usize = 8;

/// Every command buffer that failed on one device, and how many there were.
#[derive(Debug, Default)]
pub(crate) struct FaultLog {
    seen: u64,
    kept: Vec<String>,
}

impl FaultLog {
    /// Files one failure, keeping its text if there is room and counting it
    /// either way.
    fn record(&mut self, text: String) {
        self.seen += 1;
        if self.kept.len() < MAX_KEPT_FAULTS {
            self.kept.push(text);
        }
    }

    /// How many command buffers have failed on this device.
    pub(crate) const fn seen(&self) -> u64 {
        self.seen
    }

    /// One indented line per kept failure, for the assertion that quotes them.
    #[cfg(test)]
    fn summary(&self) -> String {
        self.kept
            .iter()
            .map(|fault| format!("  {fault}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Moves every command buffer that has finished out of `in_flight`, filing the
/// ones that failed.
///
/// **This is the only thing that notices a submission nobody waited on.** A
/// failed `MTLCommandBuffer` reports through `status` and `error` and through
/// nothing else — no callback, no exception, no failed later call — so a
/// submission whose result is never read fails in total silence. `poll_readback`
/// and `wait_idle` catch the ones they happen to be waiting for; everything
/// else was invisible until this.
///
/// Called before each submission and again at teardown, so the retained set is
/// bounded by what is genuinely still running rather than by the length of the
/// run.
pub(crate) fn sweep(
    in_flight: &mut Vec<Retained<ProtocolObject<dyn MTLCommandBuffer>>>,
    faults: &mut FaultLog,
) {
    in_flight.retain(|command_buffer| match command_buffer.status() {
        MTLCommandBufferStatus::Error => {
            let text = describe(command_buffer);
            crcbl_core::log::error!("crcbl-mtl: a submitted command buffer failed: {text}");
            faults.record(text);
            false
        }
        MTLCommandBufferStatus::Completed => false,
        // Not started, enqueued, committed or scheduled: still this device's
        // problem, so it stays.
        _ => true,
    });
}

/// Whether a class name is Metal's validating wrapper.
///
/// The whole of the "was the layer interposed" test, kept as a pure function on
/// the name so that the one fragile assumption in this module is in one place
/// and is stated. See the module docs for why a private class name is a better
/// oracle here than the environment variable this process exported.
fn layer_wrapped_device(class_name: &str) -> bool {
    class_name.contains("Debug")
}

/// Whether *this suite* requires Metal's validation layer.
///
/// Separate from [`DEBUG_LAYER_ENV_VAR`], which is Metal's own switch and can
/// only be set before the process starts: this one is the assertion's, and it is
/// what makes "the layer is missing" a stated choice rather than a silent
/// downgrade. Debug builds require it, release builds do not, and the variable
/// beats both — `CRCBL_VK_VALIDATION` and `CRCBL_DX12_VALIDATION` in the other
/// two backends, spelled the same way for the same reason.
#[cfg(test)]
pub(crate) const VALIDATION_ENV_VAR: &str = "CRCBL_MTL_VALIDATION";

/// Whether a run that did not get the layer should fail.
#[cfg(test)]
fn validation_required() -> bool {
    validation_policy(env_flag(VALIDATION_ENV_VAR))
}

/// The pure half of [`validation_required`], so the *default* is testable.
///
/// It has to be reachable without the environment: a test that set
/// [`VALIDATION_ENV_VAR`] would be setting it for every other test in the
/// binary, and one that only asserts on [`parse_flag`] never reaches the
/// fallback at all.
#[cfg(test)]
const fn validation_policy(override_: Option<bool>) -> bool {
    match override_ {
        Some(explicit) => explicit,
        None => cfg!(debug_assertions),
    }
}

/// Parses a boolean environment variable, tolerating the spellings people
/// actually type. An unset or empty variable is "no opinion".
fn env_flag(name: &str) -> Option<bool> {
    parse_flag(&std::env::var(name).ok()?)
}

/// The pure half of [`env_flag`], so the spelling table is testable.
fn parse_flag(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" => None,
        "0" | "false" | "no" | "off" => Some(false),
        _ => Some(true),
    }
}

/// Whether an environment variable was set to something readable as on. An
/// unset one is off, which for Metal's own switches is exactly right: Metal
/// treats absence as "not enabled".
fn env_on(name: &str) -> bool {
    env_flag(name).unwrap_or(false)
}

/// What validation this device is actually running under, and what it caught.
///
/// The Metal counterpart of `crcbl_vk::debug`'s `ValidationReport` and
/// `crcbl_dx12::debug`'s, and deliberately a different shape, because Metal
/// reports different things. See the module docs.
#[derive(Debug)]
pub(crate) struct ValidationReport {
    /// Whether [`DEBUG_LAYER_ENV_VAR`] asked for API validation.
    pub(crate) api_validation_requested: bool,
    /// Whether Metal actually wrapped this device, which is the only evidence
    /// that the request landed.
    pub(crate) layer_installed: bool,
    /// The device object's Objective-C class, so a failure names what it saw.
    pub(crate) device_class: String,
    /// Whether [`SHADER_VALIDATION_ENV_VAR`] asked for GPU-side checking.
    /// **Not confirmable**: Metal exposes nothing that says whether it took, and
    /// a device that cannot support it says so on stderr and carries on.
    pub(crate) shader_validation_requested: bool,
    /// What a validation error and a validation warning are set to do.
    pub(crate) error_mode: String,
    pub(crate) warning_mode: String,
    /// How many of this device's command buffers failed.
    pub(crate) faults_seen: u64,
    /// The first few of them, one indented line each. Only the assertion reads
    /// it — the production log line carries the count, and `sweep` has already
    /// logged each failure's text as it happened.
    #[cfg(test)]
    faults: String,
}

impl ValidationReport {
    /// Reads the switches and the device's class, and folds in what
    /// [`sweep`] has filed.
    pub(crate) fn of(device: &ProtocolObject<dyn MTLDevice>, faults: &FaultLog) -> Self {
        let object: &AnyObject = device.as_ref();
        let device_class = object.class().name().to_string_lossy().into_owned();
        Self {
            api_validation_requested: env_on(DEBUG_LAYER_ENV_VAR),
            layer_installed: layer_wrapped_device(&device_class),
            device_class,
            shader_validation_requested: env_on(SHADER_VALIDATION_ENV_VAR),
            error_mode: std::env::var(ERROR_MODE_ENV_VAR).unwrap_or_else(|_| "unset".to_string()),
            warning_mode: std::env::var(WARNING_MODE_ENV_VAR)
                .unwrap_or_else(|_| "unset".to_string()),
            faults_seen: faults.seen(),
            #[cfg(test)]
            faults: faults.summary(),
        }
    }

    /// The one line `crates/crcbl-mtl/tests/run-mtl-e2e.sh` reads this run's
    /// verdict off. **Keep the spelling**: that harness greps it and fails when
    /// it is absent, so a green run cannot claim evidence it does not have.
    pub(crate) fn line(&self) -> String {
        format!(
            "api validation={} (asked for={}) on `{}` shader validation={} error mode={} \
             warning mode={} failed submissions={}",
            self.layer_installed,
            self.api_validation_requested,
            self.device_class,
            self.shader_validation_requested,
            self.error_mode,
            self.warning_mode,
            self.faults_seen,
        )
    }

    /// Fails the calling test unless validation was interposed and nothing it
    /// can report has reported.
    ///
    /// **This is not parity with `crcbl-vk`.** It cannot be: see the module
    /// docs. An API misuse never reaches here — the layer prints it and, at
    /// every error mode but `ignore`, ends the process, so it is the runner that
    /// reports it. What is asserted is the half that is otherwise invisible: that
    /// the checking was switched on at all, and that no submission failed
    /// unnoticed.
    ///
    /// # Panics
    ///
    /// If Metal did not wrap this device, or if any of its command buffers
    /// failed.
    #[cfg(test)]
    pub(crate) fn assert_clean(&self) {
        self.assert_validated();
        self.assert_no_faults();
    }

    /// Fails unless Metal interposed its validation layer on this device.
    ///
    /// The half that fails today, and the one worth the awkwardness of reading
    /// a private class name: a suite that passed because nobody exported
    /// [`DEBUG_LAYER_ENV_VAR`] proves nothing about API misuse, and nothing else
    /// in a green log says so.
    ///
    /// # Panics
    ///
    /// If the device's class is not Metal's validating wrapper.
    #[cfg(test)]
    pub(crate) fn assert_validated(&self) {
        assert!(
            self.layer_installed,
            "Metal did not wrap this device in its validation layer — its class is `{}` — so this \
             proves nothing about API misuse. {DEBUG_LAYER_ENV_VAR} was {}. Run through \
             crates/crcbl-mtl/tests/run-mtl-e2e.sh, which exports it, or set \
             {VALIDATION_ENV_VAR}=0 to state plainly that this run checks no validation.",
            self.device_class,
            if self.api_validation_requested {
                "set, so either Metal ignored it or this macOS names the wrapper differently"
            } else {
                "not set"
            },
        );
    }

    /// Fails if any command buffer this device submitted ended in
    /// [`MTLCommandBufferStatus::Error`].
    ///
    /// Asserted on **every** run, including one that asked for no validation:
    /// a failed submission is a failed submission, and this is the only place
    /// one that nobody waited on is ever noticed.
    ///
    /// # Panics
    ///
    /// If [`sweep`] filed anything.
    #[cfg(test)]
    pub(crate) fn assert_no_faults(&self) {
        assert!(
            self.faults_seen == 0,
            "{} command buffer(s) submitted to this device failed:\n{}",
            self.faults_seen,
            self.faults,
        );
    }
}

/// A device test's instance, which fails the test at teardown unless Metal's
/// validation layer was interposed and nothing it can report reported.
///
/// The Metal counterpart of `crcbl_dx12::debug`'s `Validated` and of
/// `crcbl-vk`'s `Headless::finish`, and the same argument applies: this crate's
/// device tests are ordinary `#[test]` functions in `src/` opening through
/// `crate::device::tests::open_device`, so the only line all of them run is the
/// drop of what that returned.
///
/// It holds the device's shared state by `Arc` rather than holding the
/// [`MetalDevice`](crate::MetalDevice), so the fault log outlives the device the
/// test dropped — a `let (_instance, device) = open_device();` drops `device`
/// first, locals going in reverse declaration order.
#[cfg(test)]
pub(crate) struct Validated {
    instance: crate::MetalInstance,
    device: std::sync::Arc<crate::device::DeviceInner>,
}

#[cfg(test)]
impl Validated {
    /// Wraps the instance a device was opened on, keeping that device's state
    /// alive for the report.
    pub(crate) fn new(instance: crate::MetalInstance, device: &crate::MetalDevice) -> Self {
        Self {
            instance,
            device: std::sync::Arc::clone(&device.inner),
        }
    }

    /// Waits for whatever this device still has running, then reports.
    pub(crate) fn report(&self) -> ValidationReport {
        self.drain();
        let mut state = self.device.state();
        state.sweep();
        ValidationReport::of(&self.device.raw, &state.faults)
    }

    /// Blocks until every submission this device made has finished, so their
    /// statuses are final.
    ///
    /// A command buffer committed now completes only after everything committed
    /// before it — Metal's own ordering on a single queue, the same idiom
    /// `Device::wait_idle` uses. Skipped entirely when nothing is running, which
    /// is most of this crate's device tests.
    fn drain(&self) {
        let running = !self.device.state().in_flight.is_empty();
        if !running {
            return;
        }
        let Some(command_buffer) = command_buffer(&self.device.queue, "crcbl validation drain")
        else {
            return;
        };
        command_buffer.commit();
        command_buffer.waitUntilCompleted();
    }
}

#[cfg(test)]
impl core::ops::Deref for Validated {
    type Target = crate::MetalInstance;

    /// So that a test written against `MetalInstance` needs no edit to gain the
    /// assertion.
    fn deref(&self) -> &Self::Target {
        &self.instance
    }
}

#[cfg(test)]
impl Drop for Validated {
    fn drop(&mut self) {
        // A panic raised while another is unwinding aborts the process, taking
        // the real failure's message with it.
        if std::thread::panicking() {
            return;
        }
        let report = self.report();
        if validation_required() {
            report.assert_clean();
        } else {
            // The layer was deliberately not required, so only the half that
            // does not depend on it is asserted.
            report.assert_no_faults();
        }
    }
}

/// A labelled command buffer that reports per-encoder status when it fails.
///
/// `None` only when Metal returned nil, which is the caller's to report — the
/// two call sites word it differently and both are already saying it.
pub(crate) fn command_buffer(
    queue: &ProtocolObject<dyn MTLCommandQueue>,
    label: &str,
) -> Option<Retained<ProtocolObject<dyn MTLCommandBuffer>>> {
    let descriptor = MTLCommandBufferDescriptor::new();
    descriptor.setErrorOptions(MTLCommandBufferErrorOption::EncoderExecutionStatus);
    // `retainedReferences` is left at its default of `true`: an unretained
    // command buffer requires the caller to keep every resource it touches
    // alive itself, and the seam's handles are destroyed by whoever holds them.
    let raw = queue.commandBufferWithDescriptor(&descriptor)?;
    raw.setLabel(Some(&NSString::from_str(label)));
    Some(raw)
}

/// Why a command buffer failed, with each of its encoders' fate beside it.
///
/// Called only on a command buffer whose `status` is
/// [`MTLCommandBufferStatus::Error`](objc2_metal::MTLCommandBufferStatus::Error),
/// where Metal guarantees an `NSError`; the "no reason given" arm is the
/// defensive one and should never be reached.
pub(crate) fn describe(command_buffer: &ProtocolObject<dyn MTLCommandBuffer>) -> String {
    let Some(error) = command_buffer.error() else {
        return "no reason given".to_string();
    };
    // The domain and code are carried alongside the localised text because they
    // are the machine-readable half: `MTLCommandBufferErrorTimeout` and
    // `MTLCommandBufferErrorPageFault` are different bugs with similar prose.
    //
    // The `MTLDevice`'s own name is here for a reason a local reader will not
    // feel: a fault report is read from a CI log, on a machine nobody can open,
    // and "which GPU" is the first question a fault that reproduces nowhere
    // else raises. A virtualised device answers it in one word.
    let mut out = format!(
        "{} [{} {}] on `{}`",
        error.localizedDescription(),
        error.domain(),
        error.code(),
        command_buffer.device().name()
    );
    match encoders(&error) {
        Some(encoders) if !encoders.is_empty() => {
            out.push_str("; encoders in recorded order: ");
            out.push_str(&encoders.join(", "));
        }
        // Not silence: "Metal named no encoder" and "this backend forgot to ask
        // for them" look identical in a log otherwise, and only one of them is
        // something to go and fix.
        _ => out.push_str(
            "; no per-encoder status in the error's userInfo, though this backend creates every \
             command buffer with MTLCommandBufferErrorOptionEncoderExecutionStatus",
        ),
    }
    out
}

/// One labelled state per encoder, in recording order, or `None` when the
/// error carries no encoder array at all.
fn encoders(error: &NSError) -> Option<Vec<String>> {
    // SAFETY: `objc2` declares this as an `extern "C"` static, which Rust
    // requires an `unsafe` block to name. It is an immutable `NSString`
    // constant the Metal framework has initialised before any Metal call can
    // return, and reading the reference is the whole of the access.
    let key = unsafe { MTLCommandBufferEncoderInfoErrorKey };
    let value = error.userInfo().objectForKey(key)?;
    // Apple documents the value as an `NSArray`; the class is checked rather
    // than assumed, because a userInfo dictionary is a bag of `id` and this is
    // the boundary where that stops being true.
    let infos = value.downcast_ref::<NSArray<AnyObject>>()?;
    let protocol = AnyProtocol::get(c"MTLCommandBufferEncoderInfo")?;
    let mut out = Vec::with_capacity(infos.count());
    for index in 0..infos.count() {
        let element = infos.objectAtIndex(index);
        if !element.class().conforms_to(protocol) {
            // Reported rather than skipped: a silently dropped element would
            // shift every later encoder's position in a list whose order is
            // the recording order, which is most of what it is read for.
            out.push(format!(
                "<a {}, which does not conform to MTLCommandBufferEncoderInfo>",
                element.class().name().to_string_lossy()
            ));
            continue;
        }
        // SAFETY: the element's class was just asked whether it conforms to
        // `MTLCommandBufferEncoderInfo`, which is the same question
        // `ProtocolObject::from_ref` answers at compile time for a type known
        // to implement it — and the only question this cast turns on, since a
        // `ProtocolObject` is a type-erased object header either way.
        let info: Retained<ProtocolObject<dyn MTLCommandBufferEncoderInfo>> =
            unsafe { Retained::cast_unchecked(element) };
        // **`debugSignposts` is deliberately not read**, and the reason is a
        // trap rather than a preference. `objc2` generates it as returning a
        // non-optional `Retained<NSArray<NSString>>`, but the real
        // `_MTLCommandBufferEncoderInfo` returns **nil** when an encoder
        // recorded no signposts — which is every encoder this backend produces,
        // since nothing here calls `insertDebugSignpost:`. Sending it therefore
        // panics inside the binding with "unexpected NULL returned", *replacing*
        // the GPU fault this function exists to report with an unrelated one.
        // Measured: it did exactly that on the macOS runner and hid the fault
        // for a whole CI round trip. If signposts are ever inserted, read this
        // through a nil-tolerant path rather than the generated accessor.
        out.push(format!("`{}` {}", info.label(), state(info.errorState())));
    }
    Some(out)
}

/// An [`MTLCommandEncoderErrorState`] in the words a reader needs.
///
/// Spelled out rather than printed as a number: the one thing a fault report
/// has to make obvious is which encoder is the *cause* and which were merely
/// caught up in it, and `4` versus `2` does not say that to anyone.
fn state(state: MTLCommandEncoderErrorState) -> String {
    match state {
        MTLCommandEncoderErrorState::Completed => "completed".to_string(),
        MTLCommandEncoderErrorState::Faulted => {
            "FAULTED — this encoder caused the error".to_string()
        }
        MTLCommandEncoderErrorState::Affected => "affected by another encoder's error".to_string(),
        MTLCommandEncoderErrorState::Pending => "never started".to_string(),
        MTLCommandEncoderErrorState::Unknown => "status unknown".to_string(),
        // A state added after this was written still reaches the log by number,
        // which is worth more than a match arm that silently calls it unknown.
        other => format!("error state {}, which this build does not name", other.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_foundation::NSDictionary;

    /// **The spellings a person actually types, and the ones that mean "no
    /// opinion".**
    #[test]
    fn the_validation_flag_reads_the_spellings_people_type() {
        for off in ["0", "false", "no", "off", "OFF", " False "] {
            assert_eq!(parse_flag(off), Some(false), "{off:?}");
        }
        for on in ["1", "true", "yes", "on", "ON", " anything "] {
            assert_eq!(parse_flag(on), Some(true), "{on:?}");
        }
        assert_eq!(parse_flag(""), None);
        assert_eq!(parse_flag("   "), None);
    }

    /// **The default follows the build profile, and the variable beats it both
    /// ways.**
    ///
    /// Asserted through the pure half rather than by setting the environment,
    /// because a test that set [`VALIDATION_ENV_VAR`] would set it for every
    /// other test in this binary — including the ones that open a device.
    #[test]
    fn validation_defaults_to_the_build_profile_and_the_variable_overrides_it() {
        assert_eq!(
            validation_policy(None),
            cfg!(debug_assertions),
            "an unset {VALIDATION_ENV_VAR} means 'required in debug, not in release'"
        );
        assert!(validation_policy(Some(true)));
        assert!(!validation_policy(Some(false)));
    }

    /// **The one fragile assumption in this module, stated as a test.**
    ///
    /// Metal publishes no API for "is validation interposed", so the answer is
    /// read off the device's Objective-C class: `MTLDebugDevice` when the layer
    /// is on, the driver's own class when it is not. If a macOS release renames
    /// the wrapper, this is where to look — and
    /// [`ValidationReport::assert_validated`] fails naming the class it saw
    /// rather than passing quietly, which is the reason it is worth reading a
    /// private detail at all.
    #[test]
    fn the_validating_wrapper_is_told_apart_from_a_real_devices_class() {
        assert!(layer_wrapped_device("MTLDebugDevice"));
        for real in [
            "MTLIGAccelDevice",
            "MTLIOAccelDevice",
            "AGXG13XDevice",
            "MTLParavirtualDevice",
            "_MTLDevice",
        ] {
            assert!(
                !layer_wrapped_device(real),
                "{real} is a driver's own class, not the validation wrapper"
            );
        }
    }

    /// **A fault log counts everything and quotes the first few.**
    #[test]
    fn the_fault_log_caps_what_it_quotes_and_not_what_it_counts() {
        let mut log = FaultLog::default();
        assert_eq!(log.seen(), 0);
        assert_eq!(log.summary(), "");

        let storm = MAX_KEPT_FAULTS + 5;
        for index in 0..storm {
            log.record(format!("submission {index} hung"));
        }
        assert_eq!(log.seen() as usize, storm);
        assert_eq!(log.summary().lines().count(), MAX_KEPT_FAULTS);
        assert!(
            log.summary().contains("submission 0 hung"),
            "{}",
            log.summary()
        );
    }

    /// **The line `crates/crcbl-mtl/tests/run-mtl-e2e.sh` reads this run's
    /// verdict off, and the assertion that the layer was interposed at all.**
    ///
    /// The Metal counterpart of `crcbl_dx12::debug`'s
    /// `a_fresh_device_says_whether_it_is_validated_and_is_not_already_removed`
    /// and, like it, load-bearing outside this file: the harness greps the line
    /// and fails when it is absent, so a green run cannot claim evidence it does
    /// not have.
    ///
    /// It is *printed* rather than only asserted because two of the things on it
    /// cannot be asserted at all — whether shader validation took, and what a
    /// violation would have done — and a run that does not state them leaves a
    /// reader to assume.
    ///
    /// nextest captures a passing test's stdout, so read it with
    /// `--success-output immediate`, which is what the harness passes.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_fresh_device_says_what_validation_it_is_running_under() {
        let (instance, _device) = crate::device::tests::open_device();
        let report = instance.report();
        println!("crcbl-mtl e2e: {}", report.line());
        assert_eq!(
            report.faults_seen, 0,
            "a device that was just opened has already failed a submission"
        );
    }

    /// **The two states the whole report turns on say different things, and the
    /// right things.**
    ///
    /// Transposing the [`MTLCommandEncoderErrorState::Faulted`] and
    /// [`MTLCommandEncoderErrorState::Affected`] arms is a one-line edit that
    /// compiles, keeps every encoder in the log, and names the wrong one as the
    /// cause — which is the entire question a fault report is opened to answer.
    /// Each is asserted to carry its own sense *and not the other's*, so the
    /// swap goes red twice.
    ///
    /// The states are otherwise pairwise distinct: two arms rendering to one
    /// string would make two different fates indistinguishable in the log, and a
    /// `match` where a later arm is unreachable is exactly how that happens.
    #[test]
    fn the_faulted_encoder_and_the_affected_ones_are_not_described_alike() {
        let faulted = state(MTLCommandEncoderErrorState::Faulted);
        assert!(
            faulted.contains("caused"),
            "the encoder that caused the fault must say so: {faulted}"
        );
        assert!(
            !faulted.contains("affected"),
            "the cause is being described as a bystander: {faulted}"
        );

        let affected = state(MTLCommandEncoderErrorState::Affected);
        assert!(
            affected.contains("affected by another"),
            "a bystander encoder must say it was caught up in someone else's fault: {affected}"
        );
        assert!(
            !affected.contains("caused"),
            "a bystander is being blamed for the fault: {affected}"
        );

        let named = [
            MTLCommandEncoderErrorState::Completed,
            MTLCommandEncoderErrorState::Faulted,
            MTLCommandEncoderErrorState::Affected,
            MTLCommandEncoderErrorState::Pending,
            MTLCommandEncoderErrorState::Unknown,
        ];
        let mut described: Vec<String> = named.iter().copied().map(state).collect();
        assert_eq!(described.len(), named.len(), "nothing to check");
        described.sort_unstable();
        described.dedup();
        assert_eq!(
            described.len(),
            named.len(),
            "two encoder states render to one string, so their fates are indistinguishable \
             in a log: {described:?}"
        );

        // A state this build has no name for still reaches the log carrying its
        // number, rather than being folded into "unknown" — which is a real
        // state with a real meaning. The value is one past the last named state,
        // so it is exactly what a newer Metal would add.
        let unnamed = MTLCommandEncoderErrorState(MTLCommandEncoderErrorState::Faulted.0 + 1);
        let described = state(unnamed);
        assert!(
            described.contains(&unnamed.0.to_string()),
            "an unrecognised state lost its number: {described}"
        );
        assert_ne!(
            described,
            state(MTLCommandEncoderErrorState::Unknown),
            "an unrecognised state must not be reported as Metal's own Unknown"
        );
    }

    /// **A `userInfo` with no encoder array, and one whose value is not an
    /// array, both answer `None`** — which is what makes [`describe`] print the
    /// "no per-encoder status" sentence rather than an empty list.
    ///
    /// Built from a synthetic [`NSError`] because that half of this module needs
    /// no GPU: `encoders` only reads a dictionary. **The other half genuinely
    /// cannot be reached synthetically** — an `MTLCommandBufferEncoderInfo` is a
    /// private Metal class with no public initialiser, so no test can put a real
    /// one in the array, and the `Faulted`/`Affected` rendering is pinned
    /// directly on [`state`] above instead.
    ///
    /// The last case is the one with a bug behind it: a non-conforming element
    /// is *reported in place*, not skipped, because dropping it would shift
    /// every later encoder in a list whose order is the recording order.
    #[test]
    fn a_user_info_without_a_conforming_encoder_array_says_so_rather_than_pretending() {
        let domain = NSString::from_str("crcbl-mtl test domain");
        // SAFETY: `objc2` declares this as an `extern "C"` static, which Rust
        // requires an `unsafe` block to name. It is an immutable `NSString`
        // constant the Metal framework has initialised, and reading the
        // reference is the whole of the access — the same access `encoders`
        // makes.
        let key = unsafe { MTLCommandBufferEncoderInfoErrorKey };

        // SAFETY: the dictionary really is keyed by `NSErrorUserInfoKey` and
        // holds `AnyObject` values, which is the generic pairing this
        // constructor asks the caller to guarantee. Same for each call below.
        let no_key = unsafe { NSError::errorWithDomain_code_userInfo(&domain, 1, None) };
        assert!(
            encoders(&no_key).is_none(),
            "an error with no userInfo named an encoder"
        );

        let string = NSString::from_str("not an array");
        let string_value: &AnyObject = &string;
        let not_an_array: Retained<NSDictionary<NSString, AnyObject>> =
            NSDictionary::from_slices(&[key], &[string_value]);
        // SAFETY: as above.
        let wrong_type =
            unsafe { NSError::errorWithDomain_code_userInfo(&domain, 2, Some(&not_an_array)) };
        assert!(
            encoders(&wrong_type).is_none(),
            "a userInfo value that is not an NSArray was read as one anyway"
        );

        let strangers: Retained<NSArray<NSString>> = NSArray::from_retained_slice(&[
            NSString::from_str("first"),
            NSString::from_str("second"),
        ]);
        let strangers_value: &AnyObject = &strangers;
        let with_strangers: Retained<NSDictionary<NSString, AnyObject>> =
            NSDictionary::from_slices(&[key], &[strangers_value]);
        // SAFETY: as above.
        let stranger_error =
            unsafe { NSError::errorWithDomain_code_userInfo(&domain, 3, Some(&with_strangers)) };
        let described = encoders(&stranger_error).expect("the key holds an array");
        assert_eq!(
            described.len(),
            strangers.count(),
            "an element was dropped, which shifts every later encoder out of recorded \
             order: {described:?}"
        );
        for line in &described {
            assert!(
                line.contains("does not conform to MTLCommandBufferEncoderInfo"),
                "a non-encoder was reported as an encoder: {line}"
            );
        }
    }
}
