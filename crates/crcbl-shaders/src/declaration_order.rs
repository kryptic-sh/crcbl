//! **Declaration order must equal binding order**, checked over every
//! `shaders/*.slang`.
//!
//! `docs/plan/02-vulkan-backend.md`'s third shader portability rule, and the
//! only one of the five a lint can carry. Slang's Metal target **ignores
//! `[[vk::binding]]`** and hands each resource the next index in its own flat
//! per-stage argument table, in the order the declarations appear, while
//! `crcbl-mtl` binds by ascending `(set, binding)` — see that crate's
//! `binding` module, "The index a binding lands on". The two agree only while
//! every source declares its resources in the order it numbers them.
//!
//! When `ui.slang` did not, its MSL put the constants where the vertex buffer
//! should have been: the UI vertex stage read the viewport as its vertex array,
//! every quad landed nowhere, and macOS drew a game with no text in it. Nothing
//! below the seam can catch that — reflection names the *shader's* parameter
//! names and a `BindGroupLayoutEntry` has none to compare them with — and up to
//! this module the only thing preventing a recurrence was a comment.
//!
//! # Where a push constant sorts, and why it is last
//!
//! `[[vk::push_constant]]` carries no binding number, so it has no `(set,
//! binding)` to sort on — but Slang's Metal target still gives it a slot in the
//! buffer table, taken in declaration order like everything else. It therefore
//! has to sort *somewhere*, and [`Slot`] puts it after every numbered binding.
//!
//! That is the position `crcbl-mtl` leaves free. Push constants are not a
//! binding there at all: the backend reports no
//! `Features::PUSH_CONSTANTS` and refuses a push-constant range at
//! `create_pipeline_layout`, so the indices it computes cover the numbered
//! bindings and nothing else. A push constant declared *first* would take
//! `buffer(0)` in the MSL and shift every buffer binding up by one past what
//! that backend computes; declared last it takes the first index after them,
//! and the numbered bindings keep the slots the backend assigns.
//!
//! `push_constant_probe.slang` is the one shipped source that exercises this
//! arm, and its MSL is what shows the rule doing its job: the block lands at
//! `buffer(1)`, behind the one numbered binding, exactly where
//! [`crate::push_constant_probe`] records it. Declared the other way round it
//! would take `buffer(0)` — which is what `ui.slang`'s artifact did until
//! 2026-08, and what the failing cases below reproduce. See `crcbl_shaders`'
//! crate docs for why no *other* source may reintroduce one.
//!
//! # The rule checked here is stricter than the one that must hold
//!
//! Metal counts *per table* — buffers, textures and samplers are three
//! independent index spaces — so what must actually hold is ascending
//! `(set, binding)` within each table. This checks it globally instead, which
//! implies the per-table rule and is not implied by it.
//!
//! Deliberately: a global rule is one an author follows by reading the file top
//! to bottom, and one a reviewer checks the same way, where the per-table rule
//! needs you to know which of Metal's three tables each Slang type lowers to
//! before you can tell whether a file is wrong. The cost is that a source may
//! be asked to move a declaration that would have been harmless — a one-line
//! edit, made once, at authoring time.

/// Where a resource sorts in the order the Metal target must agree with.
///
/// The derived `Ord` **is** the rule: variants compare in declaration order, so
/// every [`Bound`](Self::Bound) sorts before every
/// [`PushConstant`](Self::PushConstant), and two bound resources compare by set
/// and then by binding. The module docs say why a push constant belongs last.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Slot {
    /// A numbered binding, from `[[vk::binding(binding, set)]]`.
    ///
    /// The fields are in sort order, which is *not* the order the attribute
    /// writes them: Slang takes the binding first and the set second, so
    /// `[[vk::binding(0, 1)]]` is set 1, binding 0.
    Bound {
        /// Descriptor set.
        set: u32,
        /// Binding number within the set.
        binding: u32,
    },
    /// `[[vk::push_constant]]`, which carries no binding number.
    PushConstant,
}

impl std::fmt::Display for Slot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bound { set, binding } => write!(formatter, "set {set} binding {binding}"),
            Self::PushConstant => formatter.write_str("push constant"),
        }
    }
}

/// One resource a Slang source declares.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Resource {
    /// The declared name, as the shader spells it.
    name: String,
    /// 1-based line the attribute is on, so a failure is navigable.
    line: usize,
    /// Where it sorts.
    slot: Slot,
}

/// Every resource `source` declares, in declaration order.
///
/// # Errors
///
/// A `String` naming the line and what could not be read. Malformed is an
/// error rather than a skip: a resource this parser silently passed over is one
/// the order check never sees, which is the shape of a lint that reports a
/// broken file as clean.
fn resources(source: &str) -> Result<Vec<Resource>, String> {
    const OPEN: &str = "[[vk::";
    let text = blank_comments(source);
    let mut found = Vec::new();
    let mut cursor = 0;

    while let Some(offset) = text[cursor..].find(OPEN) {
        let start = cursor + offset;
        let body_start = start + OPEN.len();
        let Some(close) = text[body_start..].find("]]") else {
            return Err(format!(
                "line {}: `{OPEN}` is never closed",
                line_of(&text, start)
            ));
        };
        let body = text[body_start..body_start + close].trim();
        cursor = body_start + close + 2;

        // The attribute's name is what precedes its argument list, if any.
        let name = body.split('(').next().unwrap_or(body).trim();
        let slot = match name {
            "binding" => {
                let arguments = body
                    .strip_prefix("binding")
                    .and_then(|rest| rest.trim().strip_prefix('('))
                    .and_then(|rest| rest.strip_suffix(')'))
                    .ok_or_else(|| {
                        format!(
                            "line {}: `[[vk::{body}]]` takes `(binding, set)`",
                            line_of(&text, start)
                        )
                    })?;
                let mut numbers = arguments.split(',');
                // Slang's argument order, which is the reverse of the sort
                // order: the binding comes first and the set second.
                let (Some(binding), Some(set), None) =
                    (numbers.next(), numbers.next(), numbers.next())
                else {
                    return Err(format!(
                        "line {}: `[[vk::{body}]]` must name exactly a binding and a set",
                        line_of(&text, start)
                    ));
                };
                let number = |value: &str, what: &str| -> Result<u32, String> {
                    value.trim().parse::<u32>().map_err(|error| {
                        format!(
                            "line {}: `[[vk::{body}]]`'s {what} is not a number ({error})",
                            line_of(&text, start)
                        )
                    })
                };
                Slot::Bound {
                    set: number(set, "set")?,
                    binding: number(binding, "binding")?,
                }
            }
            "push_constant" => Slot::PushConstant,
            // Every other `[[vk::…]]` — `location`, `image_format`, an
            // attribute a later Slang adds — decorates something that is not a
            // resource in an argument table, and is not this lint's business.
            _ => continue,
        };

        let Some(semicolon) = text[cursor..].find(';') else {
            return Err(format!(
                "line {}: `[[vk::{body}]]` decorates nothing that ends in `;`",
                line_of(&text, start)
            ));
        };
        let declaration = &text[cursor..cursor + semicolon];
        let name = declared_name(declaration).ok_or_else(|| {
            format!(
                "line {}: `[[vk::{body}]]` decorates `{}`, which declares no name",
                line_of(&text, start),
                declaration.trim()
            )
        })?;
        found.push(Resource {
            name,
            line: line_of(&text, start),
            slot,
        });
        cursor += semicolon + 1;
    }
    Ok(found)
}

/// Refuses a source whose resources are not declared in ascending [`Slot`]
/// order.
///
/// # Errors
///
/// A `String` naming both resources and both slots — the failure is a
/// *relationship* between two declarations, so reporting one of them would
/// leave the reader to find the other.
fn check_order(resources: &[Resource]) -> Result<(), String> {
    for pair in resources.windows(2) {
        let (first, second) = (&pair[0], &pair[1]);
        if second.slot > first.slot {
            continue;
        }
        let complaint = if second.slot == first.slot {
            format!("both are at {}", first.slot)
        } else {
            format!("{} sorts before {}", second.slot, first.slot)
        };
        return Err(format!(
            "line {}: `{}` ({}) is declared after `{}` ({}), but {complaint}.\n  Slang's Metal \
             target assigns argument-table indices in declaration order and ignores \
             `[[vk::binding]]`, while crcbl-mtl binds by ascending (set, binding) — so this \
             shader's MSL binds resources to each other's slots. Declare them in the order they \
             are numbered, push constants last.",
            second.line, second.name, second.slot, first.name, first.slot,
        ));
    }
    Ok(())
}

/// `source` with every comment's bytes replaced by spaces.
///
/// Byte length and every newline are preserved, so offsets and line numbers
/// still address the original file — and the prose in these sources talks about
/// `[[vk::binding]]` at length, which an un-stripped scan would parse as
/// declarations.
///
/// String literals are tracked because a `//` inside one does not open a
/// comment. Slang has no raw or multi-line string form, so a backslash escape
/// is the only thing that can hide a closing quote.
fn blank_comments(source: &str) -> String {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        Code,
        LineComment,
        BlockComment,
        Str,
    }

    let bytes = source.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut state = State::Code;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        let next = bytes.get(index + 1).copied();
        match state {
            State::Code => match (byte, next) {
                (b'/', Some(b'/')) => {
                    state = State::LineComment;
                    out.extend_from_slice(b"  ");
                    index += 2;
                }
                (b'/', Some(b'*')) => {
                    state = State::BlockComment;
                    out.extend_from_slice(b"  ");
                    index += 2;
                }
                _ => {
                    if byte == b'"' {
                        state = State::Str;
                    }
                    out.push(byte);
                    index += 1;
                }
            },
            State::LineComment => {
                if byte == b'\n' {
                    state = State::Code;
                    out.push(byte);
                } else {
                    out.push(b' ');
                }
                index += 1;
            }
            State::BlockComment => {
                if (byte, next) == (b'*', Some(b'/')) {
                    state = State::Code;
                    out.extend_from_slice(b"  ");
                    index += 2;
                } else {
                    out.push(if byte == b'\n' { b'\n' } else { b' ' });
                    index += 1;
                }
            }
            State::Str => {
                out.push(byte);
                if byte == b'\\' {
                    if let Some(escaped) = next {
                        out.push(escaped);
                        index += 2;
                        continue;
                    }
                } else if byte == b'"' {
                    state = State::Code;
                }
                index += 1;
            }
        }
    }
    String::from_utf8(out).expect("only whole comment regions are replaced, and only by ASCII")
}

/// The name a resource declaration declares — the last identifier before any
/// array suffix.
///
/// `ConstantBuffer<Params> params` is `params`, and
/// `Texture2D<float> atlas[4]` is `atlas`: everything from the first `[` is the
/// array bound, and the generic arguments before the name are identifiers this
/// walks past.
fn declared_name(declaration: &str) -> Option<String> {
    let head = declaration.split('[').next().unwrap_or(declaration);
    let mut last = None;
    let mut current = String::new();
    for character in head.chars() {
        if character == '_' || character.is_ascii_alphanumeric() {
            current.push(character);
        } else if !current.is_empty() {
            last = Some(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        last = Some(current);
    }
    // A leading digit means this is a number, not a name — `float4x4 m` ends in
    // an identifier, `SomeType 4` does not.
    last.filter(|name| !name.starts_with(|c: char| c.is_ascii_digit()))
}

/// The 1-based line `offset` falls on.
fn line_of(text: &str, offset: usize) -> usize {
    text[..offset].bytes().filter(|&byte| byte == b'\n').count() + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    /// Every `.slang` this crate ships, by path.
    ///
    /// The directory rather than the manifest, so a source added and not yet
    /// compiled is still linted — and the two are then cross-checked, because a
    /// sweep that read the wrong directory would find nothing and pass
    /// everything.
    fn shader_sources() -> Vec<PathBuf> {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("shaders");
        let mut sources: Vec<PathBuf> = std::fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
            .map(|entry| {
                entry
                    .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
                    .path()
            })
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "slang")
            })
            .collect();
        sources.sort();
        sources
    }

    /// **The rule.** Every shipped shader declares its resources in ascending
    /// `(set, binding)`, push constants last.
    ///
    /// Falsified by reordering two declarations in a real shader and watching
    /// this go red — and permanently by
    /// [`a_source_that_declares_out_of_order_is_refused`], which does the same
    /// thing to a source that lives here.
    #[test]
    fn every_shader_declares_its_resources_in_binding_order() {
        let sources = shader_sources();
        assert!(
            !sources.is_empty(),
            "the sweep found no shaders/*.slang at all, so it checked nothing"
        );

        let mut total = 0;
        for path in &sources {
            let text = std::fs::read_to_string(path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let declared =
                resources(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            // A file that mentions the attribute anywhere — even only in its
            // prose, which several of these do — must yield at least one
            // resource. Deliberately loose: the point is that a parser which
            // quietly returned nothing cannot pass.
            if text.contains("[[vk::") {
                assert!(
                    !declared.is_empty(),
                    "{}: the source names `[[vk::` but the parse found no resources, so this \
                     file was checked vacuously",
                    path.display()
                );
            }
            total += declared.len();
            check_order(&declared).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        }
        assert!(
            total > 0,
            "the sweep read {} shader(s) and found no resources in any of them",
            sources.len()
        );
    }

    /// The sweep covers exactly the shaders the manifest ships.
    ///
    /// Both directions. A source on disk that the manifest does not name is one
    /// nothing regenerates an artifact for; a manifest entry with no source is
    /// a path scheme that has come apart from the tree. Either way the lint
    /// above would be checking a different set of files than the engine builds.
    #[test]
    fn the_sweep_covers_every_shipped_shader() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let swept: Vec<PathBuf> = shader_sources();
        let mut shipped: Vec<PathBuf> = crate::ALL
            .iter()
            .map(|shader| root.join(shader.source()))
            .collect();
        shipped.sort();
        assert_eq!(
            swept, shipped,
            "the shaders/ directory and the manifest name different sources"
        );
    }

    /// A source in the order it numbers, including a push constant last, is
    /// accepted — so the refusals below are the rule and not a parser that
    /// rejects everything.
    #[test]
    fn a_source_in_binding_order_is_accepted() {
        let source = "\
[[vk::binding(0, 0)]]
Texture2D<float> glyphAtlas;
[[vk::binding(1, 0)]]
SamplerState glyphSampler;
[[vk::binding(0, 1)]]
StructuredBuffer<Vertex> vertices;
[[vk::push_constant]]
ConstantBuffer<UiConstants> constants;
";
        let declared = resources(source).expect("parses");
        assert_eq!(
            declared
                .iter()
                .map(|resource| resource.name.as_str())
                .collect::<Vec<_>>(),
            ["glyphAtlas", "glyphSampler", "vertices", "constants"]
        );
        assert_eq!(declared[0].slot, Slot::Bound { set: 0, binding: 0 });
        // `[[vk::binding(0, 1)]]` is set 1, binding 0 — the attribute takes the
        // binding first. Reading it the other way round would make this whole
        // lint sort by the wrong key while still passing every ordered file.
        assert_eq!(declared[2].slot, Slot::Bound { set: 1, binding: 0 });
        assert_eq!(declared[3].slot, Slot::PushConstant);
        assert_eq!(declared[3].line, 7);
        check_order(&declared).expect("in order");
    }

    /// Two declarations swapped is exactly the `ui.slang` defect, and it must
    /// be refused with both names in the message.
    #[test]
    fn a_source_that_declares_out_of_order_is_refused() {
        let source = "\
[[vk::binding(1, 0)]]
SamplerState glyphSampler;
[[vk::binding(0, 0)]]
Texture2D<float> glyphAtlas;
";
        let error = check_order(&resources(source).expect("parses")).expect_err("out of order");
        assert!(error.contains("glyphAtlas"), "{error}");
        assert!(error.contains("glyphSampler"), "{error}");
        assert!(error.contains("line 3"), "{error}");
    }

    /// A push constant declared before a numbered binding takes the buffer slot
    /// that binding was compiled for — the specific way `ui.slang` broke.
    #[test]
    fn a_push_constant_before_a_binding_is_refused() {
        let source = "\
[[vk::push_constant]]
ConstantBuffer<UiConstants> constants;
[[vk::binding(0, 0)]]
StructuredBuffer<Vertex> vertices;
";
        let error = check_order(&resources(source).expect("parses")).expect_err("push first");
        assert!(error.contains("push constant"), "{error}");
    }

    /// Two resources on one slot alias each other, which is a different defect
    /// from a swap and gets its own sentence.
    #[test]
    fn two_resources_on_one_slot_are_refused() {
        let source = "\
[[vk::binding(0, 0)]]
StructuredBuffer<Vertex> vertices;
[[vk::binding(0, 0)]]
StructuredBuffer<Vertex> shadow;
";
        let error = check_order(&resources(source).expect("parses")).expect_err("same slot");
        assert!(error.contains("both are at set 0 binding 0"), "{error}");
    }

    /// The sources here discuss `[[vk::binding]]` in their comments at length.
    /// A scan that read those would find resources in prose order, which is not
    /// an order at all.
    #[test]
    fn an_attribute_named_only_in_a_comment_is_not_a_declaration() {
        let source = "\
// Slang's Metal target ignores [[vk::binding]] and assigns
/* [[vk::push_constant]] ConstantBuffer<Nope> nope; */
[[vk::binding(0, 0)]]
StructuredBuffer<Vertex> vertices;
";
        let declared = resources(source).expect("parses");
        assert_eq!(declared.len(), 1);
        assert_eq!(declared[0].name, "vertices");
        assert_eq!(declared[0].line, 3, "the blanking must preserve lines");
    }

    /// An attribute this lint does not own decorates something that is not an
    /// argument-table resource, and is walked past rather than guessed at.
    #[test]
    fn an_unrelated_vk_attribute_is_ignored() {
        let source = "\
[[vk::location(0)]]
float4 color : COLOR;
[[vk::binding(0, 0)]]
StructuredBuffer<Vertex> vertices;
";
        let declared = resources(source).expect("parses");
        assert_eq!(declared.len(), 1);
        assert_eq!(declared[0].name, "vertices");
    }

    /// A malformed binding is an error rather than a skip. Skipping it would
    /// drop a real resource out of the ordering silently, which is the one
    /// outcome a lint must not have.
    #[test]
    fn a_malformed_binding_is_refused_rather_than_skipped() {
        for bad in [
            "[[vk::binding(0)]]\nStructuredBuffer<V> a;",
            "[[vk::binding(0, 0, 0)]]\nStructuredBuffer<V> a;",
            "[[vk::binding(zero, 0)]]\nStructuredBuffer<V> a;",
            "[[vk::binding(0, 0)]]\nStructuredBuffer<V> a",
            "[[vk::binding(0, 0)]]\n;",
        ] {
            resources(bad).expect_err(bad);
        }
    }

    /// An array of resources declares one name, and the array bound is not it.
    #[test]
    fn an_array_declaration_reports_its_name() {
        assert_eq!(
            declared_name("Texture2D<float4> atlas[4]").as_deref(),
            Some("atlas")
        );
        assert_eq!(
            declared_name("ConstantBuffer<Params> params").as_deref(),
            Some("params")
        );
        assert_eq!(declared_name("   ").as_deref(), None);
    }
}
