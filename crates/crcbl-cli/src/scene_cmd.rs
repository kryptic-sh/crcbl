//! `crcbl scene` and `crcbl edit` — a scene directory read and edited from a
//! terminal, through the editor's own document.
//!
//! `docs/plan/08-editor.md`'s architecture makes the CLI a peer of the GUI:
//! nothing editor-side may be implemented GUI-only. So every verb here is a
//! call on [`crcbl::scene_edit::Document`], the document the editor applies
//! its keys and panels through — the same validation, the same refusals, the
//! same inverse recorded — and the scene is written by the document's own
//! save. Nothing here writes a scene file.
//!
//! # The vocabulary is the editor's
//!
//! A `.scn/` chunk cannot be read without the type its rows are of, so a tool
//! that opens a scene ships a vocabulary. This one ships the editor's,
//! [`crcbl_editor::scene::vocabulary`] — its greybox block, the engine's
//! bodies and meshes, and breakout's, puppet's and towers' components —
//! rather than a list of its own that could drift from it (decided
//! 2026-10-04). The umbrella cannot hold that list: every game depends on the
//! umbrella, so naming a game there is a cycle.
//!
//! # The history outlives the process
//!
//! An edit is saved with its history beside the scene
//! ([`Document::save_with_history`]), and the next run reads it back
//! ([`Document::open_with_history`]), so `undo` walks back an edit an earlier
//! run made. `crcbl::scene_edit::history`'s module docs hold the decisions:
//! where it lives, what binds it to the scene, and its bounds.
//!
//! # An edit run holds the scene's lock
//!
//! Every edit run locks the scene ([`lock_scene`]) before it reads it and
//! lets the lock go when the process ends, so two runs, or a run and an
//! editor with the scene open, never interleave a read and a save and lose
//! an edit. A scene another program holds is refused, [`EXIT_LOCKED`],
//! rather than waited on: an editor holds its scene for as long as it has it
//! open. `list` and `query` take no lock — they write nothing, and an open
//! editor must not stop a terminal looking at its scene. `crcbl edit
//! --serve` (`crate::serve_cmd`) takes the same lock, with the same refusal,
//! and holds it until `quit`. `crcbl::scene_edit::lock`'s module docs hold
//! the lock's decisions.
//!
//! # Exit codes
//!
//! A refused edit exits `REFUSED_BASE` plus the edit protocol's own refusal
//! code ([`refusal_of`]), the code the edit server answers a client with, so a
//! script reads one set of reasons from both. A scene that will not open or
//! save is [`EXIT_FAILED`], a history that is refused is [`EXIT_HISTORY`],
//! and a scene another program holds is [`EXIT_LOCKED`].
//! `crate::scene_args::SCENE_USAGE` lists them where a user reads them.

use std::path::Path;

use crcbl::net::EditRefusal;
use crcbl::scene::scn::SceneEntityId;
use crcbl::scene_edit::{Document, EditError, SceneLock, lock_scene, refusal_of};

use crate::json::Json;
use crate::report::{EXIT_FAILED, EXIT_HISTORY, EXIT_LOCKED, Failure, Outcome, REFUSED_BASE};
use crate::scene_args::{EditArgs, SceneArgs, SceneEdit, SceneVerb};

/// The field a move writes: [`crcbl::registry::POSITION`], the one the
/// editor's gizmo and arrow keys move.
const POSITION: &str = crcbl::registry::POSITION;

/// How many axes a position has.
const AXES: usize = 3;

/// Runs `crcbl scene`.
///
/// # Errors
///
/// [`Failure`] for a scene that will not open or save, a history that is
/// refused, and an edit the document refuses — each with its exit code.
pub fn run(args: &SceneArgs) -> Result<Outcome, Failure> {
    let verb = args.verb.name();
    let vocabulary = crcbl_editor::scene::vocabulary();
    let edit = match &args.verb {
        SceneVerb::List | SceneVerb::Query { .. } => {
            // A read writes nothing, so it needs no history to open.
            let mut document = Document::open_dir(&args.dir, vocabulary)
                .map_err(|error| opening(&args.dir, verb, &error))?;
            return match &args.verb {
                SceneVerb::Query { target } => query(&mut document, target),
                _ => Ok(list(&mut document, &args.dir)),
            };
        }
        SceneVerb::Edit(edit) => edit,
    };
    let _lock = lock(&args.dir, verb)?;
    let mut document = Document::open_with_history(&args.dir, vocabulary)
        .map_err(|error| opening(&args.dir, verb, &error))?;
    let mut outcome = apply(&mut document, edit).map_err(|refused| refused.failure(verb))?;
    save(&mut document, &args.dir, verb)?;
    outcome.json.insert(0, ("verb", Json::string(verb)));
    outcome.json.extend(history_fields(&document));
    outcome.human = format!("{}; {}", outcome.human, history_line(&document));
    Ok(outcome)
}

/// Runs `crcbl edit`.
///
/// # Errors
///
/// As [`run`]: the first refused edit stops the run, saving nothing.
pub fn run_edit(args: &EditArgs) -> Result<Outcome, Failure> {
    let _lock = lock(&args.dir, "edit")?;
    let mut document = Document::open_with_history(&args.dir, crcbl_editor::scene::vocabulary())
        .map_err(|error| opening(&args.dir, "edit", &error))?;
    let mut lines = Vec::with_capacity(args.edits.len());
    for (index, edit) in args.edits.iter().enumerate() {
        match apply(&mut document, edit) {
            Ok(outcome) => lines.push(outcome.human),
            Err(mut refused) => {
                refused.message = format!(
                    "-e {} (`{}`) was refused, so nothing was saved: {}",
                    index + 1,
                    edit.name(),
                    refused.message
                );
                return Err(refused
                    .failure(edit.name())
                    .with("failed", Json::Number(count(index))));
            }
        }
    }
    save(&mut document, &args.dir, "edit")?;
    let mut json = vec![("applied", Json::Number(count(args.edits.len())))];
    json.extend(history_fields(&document));
    lines.push(history_line(&document));
    Ok(Outcome {
        human: lines.join("\n"),
        json,
    })
}

/// An edit the document refused, and what it named.
struct Refused {
    reason: EditRefusal,
    message: String,
}

impl Refused {
    /// The refusal of `error`, by the edit protocol's code for it.
    fn of(error: &EditError) -> Self {
        Self {
            reason: refusal_of(error),
            message: error.to_string(),
        }
    }

    /// A refusal no [`EditError`] carries: what the CLI resolves before the
    /// document sees anything.
    fn new(reason: EditRefusal, message: impl Into<String>) -> Self {
        Self {
            reason,
            message: message.into(),
        }
    }

    /// The failure `verb` reports this refusal as: its code's exit, and the
    /// code and its name beside the message.
    fn failure(self, verb: &'static str) -> Failure {
        let mut failure = Failure::new(self.message)
            .with("verb", Json::string(verb))
            .with("refusal", Json::Number(i64::from(self.reason.0)));
        if let Some(name) = self.reason.name() {
            failure = failure.with("reason", Json::string(name));
        }
        failure.code = refusal_exit(self.reason);
        failure
    }
}

/// The process exit code a refusal ends a run with.
fn refusal_exit(reason: EditRefusal) -> u8 {
    REFUSED_BASE.saturating_add(reason.0)
}

/// `index` as a JSON number.
fn count(index: usize) -> i64 {
    i64::try_from(index).unwrap_or(i64::MAX)
}

/// The lock on the scene at `dir`, held until the run ends, or the failure
/// that says who holds it — see the module docs.
pub(crate) fn lock(dir: &Path, verb: &'static str) -> Result<SceneLock, Failure> {
    lock_scene(dir).map_err(|error| {
        let mut failure = opening(dir, verb, &error);
        if let EditError::Locked {
            holder: Some(holder),
            ..
        } = &error
        {
            failure = failure.with("holder", Json::string(holder.clone()));
        }
        failure
    })
}

/// The failure a scene that would not open or lock is reported as.
pub(crate) fn opening(dir: &Path, verb: &'static str, error: &EditError) -> Failure {
    let code = match error {
        EditError::History(_) => EXIT_HISTORY,
        EditError::Locked { .. } => EXIT_LOCKED,
        _ => EXIT_FAILED,
    };
    let mut failure = Failure::new(format!("`{}`: {error}", dir.display()))
        .with("verb", Json::string(verb))
        .with("dir", Json::string(dir.display().to_string()));
    failure.code = code;
    failure
}

/// The document saved with its history, or the failure that says why not.
fn save(document: &mut Document, dir: &Path, verb: &'static str) -> Result<(), Failure> {
    document.save_with_history().map_err(|error| {
        Failure::new(format!("`{}` would not be saved: {error}", dir.display()))
            .with("verb", Json::string(verb))
            .with("dir", Json::string(dir.display().to_string()))
    })
}

/// Applies `edit` to `document`, and says what it did.
fn apply(document: &mut Document, edit: &SceneEdit) -> Result<Outcome, Refused> {
    let refused = |error: EditError| Refused::of(&error);
    match edit {
        SceneEdit::Spawn { system, sets } => {
            let fields: Vec<(&str, &str)> = sets
                .iter()
                .map(|(path, value)| (path.as_str(), value.as_str()))
                .collect();
            let id = document.spawn_with(system, &fields).map_err(refused)?;
            Ok(edited(format!("spawned #{id} in `{system}`"), id))
        }
        SceneEdit::Set {
            entity,
            path,
            value,
            system,
        } => {
            let id = resolve(document, entity)?;
            let system = match system {
                Some(system) => system.clone(),
                None => only_system(document, id)?,
            };
            document
                .paste_field(id, &system, path, value)
                .map_err(refused)?;
            let now = document.copy_field(id, &system, path).map_err(refused)?;
            Ok(edited(
                format!("set #{id}'s `{path}` in `{system}` to {now}"),
                id,
            ))
        }
        SceneEdit::Delete { entity } => {
            let id = resolve(document, entity)?;
            document.delete(&[id]).map_err(refused)?;
            Ok(edited(format!("deleted #{id}"), id))
        }
        SceneEdit::Move { entity, to } => {
            let id = resolve(document, entity)?;
            let system = document.placing_system(id).ok_or_else(|| {
                Refused::new(
                    EditRefusal::UNKNOWN_PATH,
                    format!("#{id} is not a thing in space, so nothing moves it"),
                )
            })?;
            let paths: Vec<String> = (0..AXES).map(|axis| format!("{POSITION}.{axis}")).collect();
            let fields: Vec<(&str, &str)> = paths
                .iter()
                .zip(to)
                .map(|(path, value)| (path.as_str(), value.as_str()))
                .collect();
            document
                .paste_fields(id, &system, &fields)
                .map_err(refused)?;
            let mut at = Vec::with_capacity(AXES);
            for path in &paths {
                at.push(document.copy_field(id, &system, path).map_err(refused)?);
            }
            Ok(edited(
                format!("moved #{id} in `{system}` to ({})", at.join(", ")),
                id,
            ))
        }
        SceneEdit::Undo => walked(document.undo(), "undo", EditRefusal::NOTHING_TO_UNDO),
        SceneEdit::Redo => walked(document.redo(), "redo", EditRefusal::NOTHING_TO_REDO),
    }
}

/// An edit's outcome: what it did, and the entity it did it to.
fn edited(human: String, id: SceneEntityId) -> Outcome {
    Outcome {
        human,
        json: vec![("entity", Json::Number(i64::from(id.0)))],
    }
}

/// An undo's or a redo's outcome, `step` being which: `nothing` when there
/// was no entry to walk.
fn walked(
    walked: Result<bool, EditError>,
    step: &str,
    nothing: EditRefusal,
) -> Result<Outcome, Refused> {
    match walked {
        Ok(true) => Ok(Outcome {
            human: format!("{step}: one edit walked"),
            json: Vec::new(),
        }),
        Ok(false) => Err(Refused::new(
            nothing,
            format!("the history has no edit to {step}"),
        )),
        Err(error) => Err(Refused::of(&error)),
    }
}

/// The entity `text` names: an id, `4` or `#4`, or an entity's name.
fn resolve(document: &mut Document, text: &str) -> Result<SceneEntityId, Refused> {
    let digits = text.strip_prefix('#').unwrap_or(text);
    if let Ok(id) = digits.parse::<u32>() {
        let id = SceneEntityId(id);
        return if document.ids().entity(id).is_some() {
            Ok(id)
        } else {
            Err(Refused::of(&EditError::NoEntity(id)))
        };
    }
    entities(document)
        .into_iter()
        .find(|&id| {
            document
                .entity_name(id)
                .is_some_and(|name| name.as_str() == text)
        })
        .ok_or_else(|| {
            Refused::new(
                EditRefusal::UNKNOWN_ENTITY,
                format!("the scene holds no entity named `{text}`"),
            )
        })
}

/// The one system holding `id`, for a `set` that named none.
fn only_system(document: &mut Document, id: SceneEntityId) -> Result<String, Refused> {
    let mut systems = document.systems_of(id);
    if systems.len() == 1 {
        return Ok(systems.remove(0));
    }
    Err(Refused::new(
        EditRefusal::UNKNOWN_SYSTEM,
        format!(
            "#{id} has components in {}; name one with --system",
            systems.join(", ")
        ),
    ))
}

/// Every entity, each once, in the outline's order.
fn entities(document: &mut Document) -> Vec<SceneEntityId> {
    document
        .outline()
        .into_iter()
        .flat_map(|(_, ids)| ids)
        .collect()
}

/// The history's position and length, for `--json`.
fn history_fields(document: &Document) -> Vec<(&'static str, Json)> {
    vec![(
        "history",
        Json::Object(vec![
            ("position", Json::Number(count(document.log().position()))),
            ("length", Json::Number(count(document.log().len()))),
        ]),
    )]
}

/// The history's position and length, for a person.
fn history_line(document: &Document) -> String {
    format!(
        "saved; history at {} of {}",
        document.log().position(),
        document.log().len()
    )
}

/// `crcbl scene list`.
fn list(document: &mut Document, dir: &Path) -> Outcome {
    let outline = document.outline();
    let mut human = vec![format!(
        "scene `{}` in {}: {} systems, {} entities",
        document.name(),
        dir.display(),
        outline.len(),
        document.entity_count()
    )];
    let mut systems = Vec::with_capacity(outline.len());
    let mut entities = Vec::new();
    for (system, ids) in outline {
        human.push(system.clone());
        let mut members = Vec::with_capacity(ids.len());
        for id in document.entities_in(&system) {
            members.push(Json::Number(i64::from(id.0)));
        }
        systems.push(Json::Object(vec![
            ("name", Json::string(system)),
            ("entities", Json::Array(members)),
        ]));
        for id in ids {
            let name = document
                .entity_name(id)
                .map(|name| name.as_str().to_owned());
            let held = document.systems_of(id);
            human.push(format!(
                "  #{id}{}{}",
                name.as_ref()
                    .map_or_else(String::new, |name| format!(" `{name}`")),
                if held.len() > 1 {
                    format!(" (also in {})", held[1..].join(", "))
                } else {
                    String::new()
                }
            ));
            entities.push(entity_json(id, name, held, None));
        }
    }
    Outcome {
        human: human.join("\n"),
        json: vec![
            ("verb", Json::string("list")),
            ("scene", Json::string(document.name())),
            ("systems", Json::Array(systems)),
            ("entities", Json::Array(entities)),
        ],
    }
}

/// `crcbl scene query`: an entity's every component's fields, or every
/// entity's in a system — a system first, since a name is the scene's and a
/// system the vocabulary's.
fn query(document: &mut Document, target: &str) -> Result<Outcome, Failure> {
    let refused = |refused: Refused| refused.failure("query");
    if document.manifest().iter().any(|system| system == target) {
        let mut human = vec![format!("system `{target}`")];
        let mut members = Vec::new();
        for id in document.entities_in(target) {
            let (lines, json) = fields(document, id, &[target.to_owned()]).map_err(refused)?;
            human.extend(lines);
            members.push(json);
        }
        return Ok(Outcome {
            human: human.join("\n"),
            json: vec![
                ("verb", Json::string("query")),
                ("system", Json::string(target)),
                ("entities", Json::Array(members)),
            ],
        });
    }
    let id = resolve(document, target).map_err(refused)?;
    let held = document.systems_of(id);
    let (lines, json) = fields(document, id, &held).map_err(refused)?;
    Ok(Outcome {
        human: lines.join("\n"),
        json: vec![("verb", Json::string("query")), ("entity", json)],
    })
}

/// `id`'s fields in each of `systems`, as lines for a person and one JSON
/// entity.
fn fields(
    document: &mut Document,
    id: SceneEntityId,
    systems: &[String],
) -> Result<(Vec<String>, Json), Refused> {
    let name = document
        .entity_name(id)
        .map(|name| name.as_str().to_owned());
    let mut lines = vec![format!(
        "#{id}{}",
        name.as_ref()
            .map_or_else(String::new, |name| format!(" `{name}`"))
    )];
    let mut components = Vec::with_capacity(systems.len());
    for system in systems {
        lines.push(format!("  {system}"));
        let texts = document
            .field_texts(id, system)
            .map_err(|error| Refused::of(&error))?;
        let mut fields = Vec::with_capacity(texts.len());
        for (path, text) in texts {
            lines.push(format!("    {path} = {text}"));
            fields.push(Json::Object(vec![
                ("path", Json::string(path)),
                ("value", Json::string(text)),
            ]));
        }
        components.push(Json::Object(vec![
            ("system", Json::string(system.clone())),
            ("fields", Json::Array(fields)),
        ]));
    }
    let json = entity_json(id, name, document.systems_of(id), Some(components));
    Ok((lines, json))
}

/// One entity as `--json` writes it: its id, its name or `null`, its
/// systems, and with `components` each one's fields.
fn entity_json(
    id: SceneEntityId,
    name: Option<String>,
    systems: Vec<String>,
    components: Option<Vec<Json>>,
) -> Json {
    let mut fields = vec![
        ("id", Json::Number(i64::from(id.0))),
        ("name", name.map_or(Json::Null, Json::string)),
        ("systems", Json::strings(systems)),
    ];
    if let Some(components) = components {
        fields.push(("components", Json::Array(components)));
    }
    Json::Object(fields)
}
