//! Commands, and the context one runs in.

use std::any::Any;
use std::fmt;

use crate::registry::Registry;
use crate::value::Fault;

/// A console command: a name, a line of help, and the function it runs.
///
/// Declared with [`concommand!`](crate::concommand) in the crate that owns the
/// behaviour, and listed once in that crate's [`Table`](crate::Table) — the same
/// arrangement a [`ConVar`](crate::ConVar) has, and for the same reason.
#[derive(Clone, Copy, Debug)]
pub struct ConCommand {
    name: &'static str,
    help: &'static str,
    run: fn(&mut Context<'_>, &[&str]) -> Result<(), Fault>,
}

impl ConCommand {
    /// A command over `run`.
    #[must_use]
    pub const fn new(
        name: &'static str,
        help: &'static str,
        run: fn(&mut Context<'_>, &[&str]) -> Result<(), Fault>,
    ) -> Self {
        Self { name, help, run }
    }

    /// The name a person types.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The one-line help, from the declaration's doc comment.
    #[must_use]
    pub const fn help(&self) -> &'static str {
        self.help
    }

    /// Run it.
    ///
    /// # Errors
    ///
    /// Whatever the command refuses, as a [`Fault`] the caller prints. A command
    /// that returns one has left the state alone.
    pub fn run(&self, cx: &mut Context<'_>, args: &[&str]) -> Result<(), Fault> {
        (self.run)(cx, args)
    }
}

/// What a command is handed: where its output goes, what it may reach, and the
/// registry it was run from.
///
/// **The output sink is a `Vec<String>` the caller drains, not a
/// `&mut dyn FnMut(&str)`.** Two reasons, and the first is the one that decided
/// it: a test asserts on what `help` printed by reading [`lines`](Self::lines),
/// with no closure, no interior mutability and no second borrow of the thing it
/// is writing into — a closure sink would have to capture a buffer the
/// assertion then has to get back out. The second is that plan decision 4 sends
/// every console line through `crcbl_core::log` as well as to the panel, so the
/// engine drains one vector into both; a callback would make the panel and the
/// terminal two separate wirings that can disagree.
///
/// `host` is `&mut dyn Any` because this crate depends on nothing and cannot
/// name the engine state a [`Binding`](crate::Binding) writes; the crate that
/// declares the binding downcasts it.
pub struct Context<'a> {
    registry: &'a Registry,
    host: &'a mut dyn Any,
    lines: Vec<String>,
    clear: bool,
}

impl<'a> Context<'a> {
    /// A context over `registry` and the host state `host`.
    ///
    /// `registry` is the table `help` and `find` read, and is the one
    /// [`Registry::execute`] should be called on — pass the same registry to
    /// both, which reads as one name used twice at the call site.
    #[must_use]
    pub fn new(registry: &'a Registry, host: &'a mut dyn Any) -> Self {
        Self {
            registry,
            host,
            lines: Vec::new(),
            clear: false,
        }
    }

    /// The registry this context was made over.
    ///
    /// Borrowed from the registry rather than from `self`, so a command can hold
    /// it and still print.
    #[must_use]
    pub fn registry(&self) -> &'a Registry {
        self.registry
    }

    /// The host state, to read.
    #[must_use]
    pub fn host(&self) -> &dyn Any {
        self.host
    }

    /// The host state, to write.
    #[must_use]
    pub fn host_mut(&mut self) -> &mut dyn Any {
        self.host
    }

    /// Emit one line.
    pub fn print(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
    }

    /// Everything printed so far, in order.
    #[must_use]
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Everything printed so far, taken.
    #[must_use]
    pub fn into_lines(self) -> Vec<String> {
        self.lines
    }

    /// Ask the UI to drop the log it is showing.
    ///
    /// A **request**, not an action: this crate owns no log and draws nothing,
    /// so `clear` records the ask here and the panel that drains this context
    /// honours it by emptying its own view. The engine's log ring is not
    /// touched — Source's `clear` empties the console, not the file.
    pub fn request_clear(&mut self) {
        self.clear = true;
    }

    /// Whether a command asked for [`request_clear`](Self::request_clear).
    #[must_use]
    pub fn clear_requested(&self) -> bool {
        self.clear
    }
}

/// Prints what a reader can act on: the output so far and the clear request.
/// `host` is `dyn Any`, which has nothing to print.
impl fmt::Debug for Context<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Context")
            .field("lines", &self.lines)
            .field("clear", &self.clear)
            .finish_non_exhaustive()
    }
}
