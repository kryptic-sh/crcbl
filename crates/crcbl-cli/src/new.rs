//! `crcbl new` — scaffold a project that builds and runs immediately.
//!
//! # Decision: a standalone crate with a path dependency, not a workspace
//!   member
//!
//! `docs/plan/11-cli-headless.md` allows either ("workspace member or
//! standalone"). The deciding constraint is the one the slice states: **a
//! template that does not compile is worse than none.** So the question is
//! what, exactly, a generated `Cargo.toml` can name today.
//!
//! * `crcbl = "0.1"` — does not resolve. The engine is not published to
//!   crates.io and will not be before the release phase. This is the shape the
//!   template *wants*, and it is one line away: the generated manifest already
//!   carries `version = "…"` next to `path = "…"` so the swap is deleting a
//!   key.
//! * A workspace member of the engine's own workspace (`apps/<name>`, which the
//!   root `members` glob already covers) — builds instantly, and forces every
//!   game to live inside an engine checkout. It also means `crcbl new` writes
//!   into the engine's source tree, which makes the CLI's own tests dirty the
//!   repository they are running in.
//! * **A standalone crate depending on the engine by path** — builds
//!   immediately, lives anywhere, and its `Cargo.toml` says out loud that the
//!   engine is a local checkout. That last part is the honest one: it *is* a
//!   local checkout, and a generated file that pretends otherwise would fail at
//!   `cargo build` instead of at `crcbl new`.
//!
//! The generated manifest declares an empty `[workspace]` table so that
//! creating a project inside some other workspace cannot silently absorb it.
//!
//! The engine checkout is found by walking up from the working directory
//! looking for `crates/crcbl/Cargo.toml`, or named outright with `--engine`.
//! Failing that is a *clean* failure with a message naming both options, which
//! is the P0-honest answer to "where is the engine": there is no installed
//! engine to find, so guessing would only move the error later.
//!
//! # Decision: no prompt that a script can hang on
//!
//! Topic 11's rule is "no interactive prompts unless a TTY is detected (and
//! never required)". This CLI has exactly one question — overwrite a non-empty
//! directory — and it is asked only when stdin *and* stderr are terminals and
//! `--json` was not passed. Everywhere else, and always in CI, it fails with
//! exit 1 and names `--force`. `--force` is what makes the prompt never
//! required.

use std::io::{IsTerminal, Write};
use std::path::{Component, Path, PathBuf};

use crate::args::NewArgs;
use crate::json::Json;
use crate::report::{Failure, Outcome};

/// The files a scaffolded project is made of.
///
/// `include_str!` rather than a `const &str` written inline so each template is
/// a real file that an editor highlights and `cargo fmt` can be pointed at.
const TEMPLATES: &[(&str, &str)] = &[
    ("Cargo.toml", include_str!("../templates/Cargo.toml.tmpl")),
    ("src/main.rs", include_str!("../templates/main.rs.tmpl")),
    ("README.md", include_str!("../templates/README.md.tmpl")),
    (".gitignore", include_str!("../templates/gitignore.tmpl")),
    (
        ".github/workflows/ci.yml",
        include_str!("../templates/ci.yml.tmpl"),
    ),
    // `docs/plan/11-cli-headless.md` asks for a scene dir. The scene *format*
    // is `crcbl-scene`'s and lands at P9; the directory is created now so the
    // path in a game's code is stable from the first commit.
    ("scenes/.gitkeep", ""),
];

/// The marker that says a directory is a Crucible checkout.
const ENGINE_MARKER: &str = "crates/crcbl/Cargo.toml";

/// The package inside a checkout that a game actually depends on.
const ENGINE_PACKAGE: &str = "crates/crcbl";

/// Scaffolds a project.
///
/// # Errors
///
/// [`Failure`] if no engine checkout was found, if the destination exists and
/// was not confirmed, or if a file could not be written.
pub fn run(args: &NewArgs) -> Result<Outcome, Failure> {
    let cwd = std::env::current_dir()
        .map_err(|error| Failure::new(format!("cannot read the current directory: {error}")))?;
    let engine = locate_engine(args.engine.as_deref(), &cwd)?;
    let parent = args.path.clone().unwrap_or(cwd);
    let root = parent.join(&args.name);

    if is_non_empty_dir(&root) && !args.force && !confirm_overwrite(&root, args.json) {
        return Err(Failure::new(format!(
            "{} already exists and is not empty; pass --force to write into it anyway",
            root.display()
        )));
    }

    // The dependency path is relative when the project and the engine share an
    // ancestor, so a generated manifest survives the pair being moved or cloned
    // together, and absolute otherwise, where relative would be a lie.
    let engine_dep = dependency_path(&root, &engine.join(ENGINE_PACKAGE));

    let mut written = Vec::with_capacity(TEMPLATES.len());
    for (relative, template) in TEMPLATES {
        let path = root.join(relative);
        if let Some(directory) = path.parent() {
            std::fs::create_dir_all(directory).map_err(|error| {
                Failure::new(format!("cannot create {}: {error}", directory.display()))
            })?;
        }
        let contents = render(template, &args.name, &engine_dep);
        std::fs::write(&path, contents)
            .map_err(|error| Failure::new(format!("cannot write {}: {error}", path.display())))?;
        written.push((*relative).to_string());
    }

    let display_root = root.display().to_string();
    Ok(Outcome {
        human: format!(
            "created {display_root}\n  engine: {}\n  next:   cd {} && crcbl run --headless",
            engine_dep,
            root.display()
        ),
        json: vec![
            ("name", Json::string(&args.name)),
            ("path", Json::string(&display_root)),
            ("package", Json::string(&args.name)),
            ("engine", Json::string(&engine_dep)),
            ("files", Json::strings(written)),
        ],
    })
}

/// Substitutes the three placeholders the templates use.
///
/// A three-key replacement rather than a templating engine, deliberately: the
/// generated files are Rust and TOML, and anything smarter would need escaping
/// rules of its own.
///
/// `{{engine}}` is the one substitution whose value the *user* supplies, via
/// `--engine` or the current directory, and it is only ever interpolated into
/// `path = "{{engine}}"` — a TOML basic string. A directory containing a quote
/// or a backslash is legal on every platform this runs on and would otherwise
/// end the string early and produce a manifest Cargo cannot parse, so it is
/// escaped for that context. `{{name}}` needs no escaping: `check_name` has
/// already restricted it to ASCII letters, digits, `-` and `_`.
fn render(template: &str, name: &str, engine: &str) -> String {
    template
        .replace("{{name}}", name)
        .replace("{{engine}}", &toml_basic_string(engine))
        .replace("{{version}}", env!("CARGO_PKG_VERSION"))
}

/// `value`, escaped for the inside of a TOML basic string (the `"…"` kind).
///
/// The escape set is TOML 1.0's: quote, backslash, the five named control
/// escapes, and `\uXXXX` for everything else below `0x20` plus `DEL`.
fn toml_basic_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            '\u{8}' => escaped.push_str("\\b"),
            '\u{c}' => escaped.push_str("\\f"),
            control if control < ' ' || control == '\u{7f}' => {
                escaped.push_str(&format!("\\u{:04X}", control as u32));
            }
            other => escaped.push(other),
        }
    }
    escaped
}

/// Finds the engine checkout, or explains what to do about it.
fn locate_engine(explicit: Option<&Path>, from: &Path) -> Result<PathBuf, Failure> {
    if let Some(path) = explicit {
        let path = absolute(path);
        return if path.join(ENGINE_MARKER).is_file() {
            Ok(path)
        } else {
            Err(Failure::new(format!(
                "{} is not a Crucible checkout (no {ENGINE_MARKER})",
                path.display()
            )))
        };
    }
    let mut candidate: Option<&Path> = Some(from);
    while let Some(directory) = candidate {
        if directory.join(ENGINE_MARKER).is_file() {
            return Ok(directory.to_path_buf());
        }
        candidate = directory.parent();
    }
    Err(Failure::new(format!(
        "no Crucible checkout found above {}\n\
         hint: run this inside a checkout, or pass --engine <DIR>. The engine \
         is not on crates.io yet, so a generated project has to point at one.",
        from.display()
    )))
}

/// The path the generated manifest should use to reach `engine`.
fn dependency_path(project: &Path, engine: &Path) -> String {
    relative_path(project, engine)
        .unwrap_or_else(|| engine.to_path_buf())
        .display()
        .to_string()
        // A Cargo manifest wants forward slashes on every platform, and a
        // Windows-generated `..\..\crates\crcbl` would additionally need every
        // backslash escaped in TOML.
        .replace('\\', "/")
}

/// `to`, expressed relative to `from`, when the two are actually near each
/// other.
///
/// Written out rather than taken from a crate: it is twenty lines, it is the
/// only path algebra this CLI does, and `pathdiff` would be a dependency.
///
/// Returns `None` when the only thing the two paths share is the filesystem
/// root. `/tmp/mygame` and `/home/dev/crcbl` are not *related*, and
/// `../../home/dev/crcbl/crates/crcbl` is both longer than the absolute path
/// and a lie about the two moving together.
fn relative_path(from: &Path, to: &Path) -> Option<PathBuf> {
    let from = absolute(from);
    let to = absolute(to);
    let mut from_parts = from.components().peekable();
    let mut to_parts = to.components().peekable();

    // Drop the shared prefix, counting how much of it is a real directory
    // rather than the root or a drive letter.
    let mut shared_directories = 0;
    while let (Some(left), Some(right)) = (from_parts.peek(), to_parts.peek()) {
        if left != right {
            break;
        }
        if matches!(left, Component::Normal(_)) {
            shared_directories += 1;
        }
        from_parts.next();
        to_parts.next();
    }
    if shared_directories == 0 {
        return None;
    }
    let mut relative = PathBuf::new();
    for component in from_parts {
        // A `..` or `.` left in the *base* means the two paths cannot be
        // related by counting: bail out and let the caller use an absolute one.
        match component {
            Component::Normal(_) => relative.push(".."),
            Component::CurDir => {}
            _ => return None,
        }
    }
    for component in to_parts {
        match component {
            Component::Normal(part) => relative.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    // Both paths identical: Cargo needs *something*, and `.` is it.
    if relative.as_os_str().is_empty() {
        relative.push(".");
    }
    Some(relative)
}

/// `path` made absolute without touching the filesystem.
///
/// `std::path::absolute` normalizes nothing on Unix, which is what is wanted:
/// resolving symlinks would produce a dependency path that does not match what
/// the user typed.
fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

fn is_non_empty_dir(path: &Path) -> bool {
    std::fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_some())
}

/// Asks, but only where a human can answer.
///
/// Returns `false` — "do not overwrite" — whenever there is nobody to ask,
/// which is every script, every CI job, and every `--json` invocation.
fn confirm_overwrite(root: &Path, json: bool) -> bool {
    if json || !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        return false;
    }
    eprint!(
        "{} is not empty. Write into it anyway? [y/N] ",
        root.display()
    );
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return false;
    }
    matches!(answer.trim(), "y" | "Y" | "yes" | "YES")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relative_dependency_path_is_produced_for_siblings() {
        let relative = relative_path(
            Path::new("/home/dev/games/mygame"),
            Path::new("/home/dev/crcbl/crates/crcbl"),
        );
        assert_eq!(
            relative,
            Some(PathBuf::from("../../crcbl/crates/crcbl")),
            "two directories under a shared ancestor"
        );
    }

    #[test]
    fn a_project_inside_the_checkout_points_back_into_it() {
        let relative = relative_path(
            Path::new("/src/crcbl/mygame"),
            Path::new("/src/crcbl/crates/crcbl"),
        );
        assert_eq!(relative, Some(PathBuf::from("../crates/crcbl")));
    }

    /// Two directories that share only `/` are not near each other, and an
    /// absolute path says so.
    #[test]
    fn unrelated_trees_get_an_absolute_path() {
        assert_eq!(
            relative_path(Path::new("/tmp/mygame"), Path::new("/home/dev/crcbl")),
            None
        );
        let path = dependency_path(Path::new("/tmp/mygame"), Path::new("/home/dev/crcbl"));
        assert_eq!(path, "/home/dev/crcbl");
    }

    #[test]
    fn dependency_paths_use_forward_slashes() {
        let path = dependency_path(
            Path::new("/home/dev/mygame"),
            Path::new("/home/dev/crcbl/crates/crcbl"),
        );
        assert!(!path.contains('\\'), "{path}");
        assert!(path.contains("crates/crcbl"), "{path}");
    }

    /// The templates are rendered, not copied: a leftover `{{…}}` in a
    /// generated file is a build failure in someone else's project.
    #[test]
    fn every_placeholder_is_substituted_in_every_template() {
        for (name, template) in TEMPLATES {
            let rendered = render(template, "mygame", "../crcbl/crates/crcbl");
            assert!(
                !rendered.contains("{{"),
                "{name} still has a placeholder:\n{rendered}"
            );
        }
    }

    /// The three placeholders exist because something uses them; a template
    /// that stopped mentioning the project name would be a copy-paste bug.
    #[test]
    fn the_manifest_names_the_project_and_the_engine() {
        let manifest = TEMPLATES
            .iter()
            .find(|(name, _)| *name == "Cargo.toml")
            .map(|(_, template)| render(template, "mygame", "../crcbl/crates/crcbl"))
            .expect("the manifest is a template");
        assert!(manifest.contains("name = \"mygame\""), "{manifest}");
        assert!(
            manifest.contains("path = \"../crcbl/crates/crcbl\""),
            "{manifest}"
        );
        assert!(
            manifest.contains("[workspace]"),
            "the project must be its own workspace root:\n{manifest}"
        );
    }

    /// `path = "{{engine}}"` is a TOML basic string, and a checkout directory
    /// is allowed to contain a quote. Unescaped, it closed the string and left
    /// a manifest Cargo refuses to parse.
    #[test]
    fn an_engine_path_with_toml_metacharacters_is_escaped() {
        let rendered = render(
            "crcbl = { path = \"{{engine}}\" }",
            "mygame",
            "/home/dev/say \"hi\"\\crcbl",
        );
        assert_eq!(
            rendered,
            r#"crcbl = { path = "/home/dev/say \"hi\"\\crcbl" }"#
        );
        // One opening and one closing quote, and nothing in between that ends
        // the string early.
        assert_eq!(
            rendered.matches(r#"""#).count() - rendered.matches(r#"\""#).count(),
            2,
            "{rendered}"
        );
    }

    /// The ordinary path is untouched, so escaping did not become a rewrite.
    #[test]
    fn an_ordinary_engine_path_is_substituted_verbatim() {
        assert_eq!(
            render("path = \"{{engine}}\"", "mygame", "../crcbl/crates/crcbl"),
            "path = \"../crcbl/crates/crcbl\""
        );
    }

    #[test]
    fn a_directory_without_the_marker_is_not_a_checkout() {
        let temporary = std::env::temp_dir().join("crcbl-cli-not-a-checkout");
        let _ = std::fs::create_dir_all(&temporary);
        assert!(locate_engine(Some(&temporary), &temporary).is_err());
    }
}
