//! The shader manifest: what was compiled, from what, by which compiler.
//!
//! This file is pulled into `build.rs` with `#[path]` as well as compiled into
//! the library, so it must stay dependency-free. It is a real module rather
//! than build-script-only code precisely so the parser is covered by
//! `cargo test` —
//! a manifest parser that silently skipped a record would turn the drift check
//! into a no-op, which is the failure this crate exists to prevent.
//!
//! # Why not TOML
//!
//! It looks like TOML on purpose, and it is deliberately *not* TOML. Parsing
//! real TOML in a build script means a build-dependency, and the shape here is
//! a flat list of sections with `key = value` lines — the subset every reader
//! already assumes. Anything richer would be a reason to reconsider; nothing
//! here wants to be.

/// One compiled DXIL container, and the entry point it holds.
///
/// **DXIL is the one target with an artifact per entry point rather than per
/// shader.** `dxc` compiles a single `-E`, and a D3D12 pipeline state object
/// takes one bytecode blob per stage, so a container carrying two entry points
/// would be something no call could consume. The path and the digest therefore
/// arrive on one manifest line together with the entry point they belong to,
/// where the other targets use a `path` key and a matching `-sha256` key that
/// can drift apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DxilArtifact {
    /// Entry point the container holds, as it appears in `entry-points`.
    pub entry_point: String,
    /// Artifact path, relative to the crate root.
    pub path: String,
    /// SHA-256 of the artifact, lower-case hex.
    pub sha256: String,
}

/// One shader source and the artifact built from it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShaderRecord {
    /// Section name, and the identifier the generated table uses.
    pub name: String,
    /// Source path, relative to the crate root.
    pub source: String,
    /// SHA-256 of the source file, lower-case hex. **The drift check.**
    pub source_sha256: String,
    /// Compiled SPIR-V artifact path, relative to the crate root.
    pub spirv: String,
    /// SHA-256 of the SPIR-V artifact, lower-case hex.
    pub spirv_sha256: String,
    /// Compiled WGSL artifact path, relative to the crate root. Empty when
    /// this shader has no WGSL output (pre-P5 artifacts).
    pub wgsl: String,
    /// SHA-256 of the WGSL artifact, lower-case hex. Empty when `wgsl` is.
    pub wgsl_sha256: String,
    /// Compiled MSL artifact path, relative to the crate root. Empty when this
    /// shader has no MSL output (pre-P14 artifacts).
    pub msl: String,
    /// SHA-256 of the MSL artifact, lower-case hex. Empty when `msl` is.
    pub msl_sha256: String,
    /// Compiled DXIL containers, one per entry point, in `entry-points` order.
    /// Empty when this shader has no DXIL output (pre-DX4 artifacts).
    pub dxil: Vec<DxilArtifact>,
    /// Entry points the artifact exposes, as `(name, stage)`.
    pub entry_points: Vec<(String, String)>,
}

/// Everything `spirv/manifest.txt` says.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Manifest {
    /// The exact `slangc` version the artifacts were produced by.
    ///
    /// Pinned, and compared before any byte-for-byte check: two Slang releases
    /// legitimately emit different SPIR-V for the same source, so comparing
    /// bytes across versions would fail for a reason that is not drift.
    pub slangc_version: String,
    /// The `-profile` the artifacts were produced with.
    pub target: String,
    /// What `dxc --version` reported for the compiler that produced the DXIL,
    /// with the loaded library's name stripped. Empty for a manifest with no
    /// DXIL column at all; required as soon as one shader has one.
    ///
    /// Pinned for the reason [`slangc_version`](Self::slangc_version) is, and
    /// one more: the `dxc` builds distributions ship are Shader Model 6.10
    /// preview branches that crash on a trivial shader, so an unpinned compiler
    /// here does not produce different bytes — it produces none.
    pub dxc_version: String,
    /// The shader model the DXIL was compiled at, as `dxc -T` spells it
    /// (`6_6`). Empty when `dxc_version` is.
    pub dxil_model: String,
    /// One record per shader, in file order.
    pub shaders: Vec<ShaderRecord>,
}

/// Parses a manifest, or says which line it gave up on.
///
/// # Errors
///
/// A `String` naming the 1-based line number and what was wrong with it.
pub fn parse_manifest(text: &str) -> Result<Manifest, String> {
    let mut manifest = Manifest::default();
    let mut current: Option<(ShaderRecord, usize)> = None;

    for (index, raw) in text.lines().enumerate() {
        let line_number = index + 1;
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }

        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            if let Some((record, start)) = current.take() {
                manifest.shaders.push(finish(record, start)?);
            }
            let name = name.trim();
            check_section_name(name, line_number)?;
            if manifest.shaders.iter().any(|s| s.name == name) {
                return Err(format!(
                    "line {line_number}: section [{name}] is declared twice; the generated table \
                     would have two `pub static`s of the same name"
                ));
            }
            current = Some((
                ShaderRecord {
                    name: name.to_string(),
                    source: String::new(),
                    source_sha256: String::new(),
                    spirv: String::new(),
                    spirv_sha256: String::new(),
                    wgsl: String::new(),
                    wgsl_sha256: String::new(),
                    msl: String::new(),
                    msl_sha256: String::new(),
                    dxil: Vec::new(),
                    entry_points: Vec::new(),
                },
                line_number,
            ));
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            return Err(format!(
                "line {line_number}: expected `key = value`, got {line:?}"
            ));
        };
        let (key, value) = (key.trim(), value.trim());

        match (current.as_mut().map(|(record, _)| record), key) {
            (None, "slangc-version") => manifest.slangc_version = value.to_string(),
            (None, "target") => manifest.target = value.to_string(),
            (None, "dxc-version") => manifest.dxc_version = value.to_string(),
            (None, "dxil-model") => manifest.dxil_model = value.to_string(),
            (None, other) => {
                return Err(format!(
                    "line {line_number}: `{other}` appears before any [section], and is not a \
                     header key"
                ));
            }
            (Some(record), "source") => {
                set_once(&mut record.source, value.to_string(), key, line_number)?
            }
            (Some(record), "source-sha256") => {
                let hash = hex(value, line_number)?;
                set_once(&mut record.source_sha256, hash, key, line_number)?;
            }
            (Some(record), "spirv") => {
                set_once(&mut record.spirv, value.to_string(), key, line_number)?
            }
            (Some(record), "spirv-sha256") => {
                let hash = hex(value, line_number)?;
                set_once(&mut record.spirv_sha256, hash, key, line_number)?;
            }
            (Some(record), "wgsl") => {
                set_once(&mut record.wgsl, value.to_string(), key, line_number)?
            }
            (Some(record), "wgsl-sha256") => {
                let hash = hex(value, line_number)?;
                set_once(&mut record.wgsl_sha256, hash, key, line_number)?;
            }
            (Some(record), "msl") => {
                set_once(&mut record.msl, value.to_string(), key, line_number)?
            }
            (Some(record), "msl-sha256") => {
                let hash = hex(value, line_number)?;
                set_once(&mut record.msl_sha256, hash, key, line_number)?;
            }
            (Some(record), "dxil") => {
                let mut fields = value.split(':');
                let (Some(entry_point), Some(path), Some(sha256), None) =
                    (fields.next(), fields.next(), fields.next(), fields.next())
                else {
                    return Err(format!(
                        "line {line_number}: DXIL artifact {value:?} must be \
                         `entry-point:path:sha256`"
                    ));
                };
                let (entry_point, path) = (entry_point.trim(), path.trim());
                if entry_point.is_empty() || path.is_empty() {
                    return Err(format!(
                        "line {line_number}: DXIL artifact {value:?} names an empty entry point \
                         or path"
                    ));
                }
                if record
                    .dxil
                    .iter()
                    .any(|artifact| artifact.entry_point == entry_point)
                {
                    return Err(format!(
                        "line {line_number}: entry point `{entry_point}` has two DXIL artifacts; \
                         one of them would be unreachable"
                    ));
                }
                let sha256 = hex(sha256.trim(), line_number)?;
                record.dxil.push(DxilArtifact {
                    entry_point: entry_point.to_string(),
                    path: path.to_string(),
                    sha256,
                });
            }
            (Some(record), "entry-points") => {
                for pair in value.split(',') {
                    let pair = pair.trim();
                    if pair.is_empty() {
                        continue;
                    }
                    let Some((name, stage)) = pair.split_once(':') else {
                        return Err(format!(
                            "line {line_number}: entry point {pair:?} must be `name:stage`"
                        ));
                    };
                    record
                        .entry_points
                        .push((name.trim().to_string(), stage.trim().to_string()));
                }
            }
            (Some(record), other) => {
                return Err(format!(
                    "line {line_number}: unknown key `{other}` in section [{}]",
                    record.name
                ));
            }
        }
    }

    if let Some((record, start)) = current.take() {
        manifest.shaders.push(finish(record, start)?);
    }
    if manifest.slangc_version.is_empty() {
        return Err("the manifest names no `slangc-version`".to_string());
    }
    if manifest.shaders.is_empty() {
        return Err("the manifest describes no shaders".to_string());
    }
    // The version pin is what makes a byte-for-byte DXIL comparison mean
    // anything, so a manifest that has DXIL and does not say which compiler
    // produced it has recorded artifacts nothing can re-derive.
    if manifest
        .shaders
        .iter()
        .any(|shader| !shader.dxil.is_empty())
    {
        for (key, value) in [
            ("dxc-version", &manifest.dxc_version),
            ("dxil-model", &manifest.dxil_model),
        ] {
            if value.is_empty() {
                return Err(format!(
                    "the manifest records DXIL artifacts but no `{key}`, so nothing says which \
                     compiler produced them"
                ));
            }
        }
    }
    Ok(manifest)
}

/// Everything after an unquoted `#` that starts a comment.
///
/// A `#` only opens a comment at the start of a line or after whitespace, so a
/// value that legitimately contains one — a path, a fragment — survives.
/// `split('#').next()` truncated those silently.
fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    for (index, &byte) in bytes.iter().enumerate() {
        if byte == b'#' && (index == 0 || bytes[index - 1].is_ascii_whitespace()) {
            return &line[..index];
        }
    }
    line
}

/// Rejects a section name the generated table could not turn into one valid,
/// unique Rust identifier.
///
/// `build.rs` builds the identifier as `name.to_uppercase().replace('-', "_")`.
/// With no check, `2d blur` produced an invalid ident and the error surfaced
/// inside generated code rather than naming the manifest line that caused it.
fn check_section_name(name: &str, line_number: usize) -> Result<(), String> {
    if name.is_empty() {
        return Err(format!("line {line_number}: empty section name"));
    }
    if name.starts_with(|c: char| c.is_ascii_digit()) {
        return Err(format!(
            "line {line_number}: section name {name:?} starts with a digit, so it cannot become a \
             Rust identifier"
        ));
    }
    if let Some(bad) = name
        .chars()
        .find(|&c| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
    {
        return Err(format!(
            "line {line_number}: section name {name:?} contains {bad:?}; only ASCII letters, \
             digits, `-` and `_` become a Rust identifier"
        ));
    }
    Ok(())
}

/// Assigns a field once, rather than letting a repeated key silently win.
fn set_once(
    field: &mut String,
    value: String,
    key: &str,
    line_number: usize,
) -> Result<(), String> {
    if !field.is_empty() {
        return Err(format!(
            "line {line_number}: `{key}` is set twice in the same section"
        ));
    }
    *field = value;
    Ok(())
}

/// Rejects a section that is missing a field, rather than letting an empty
/// hash compare equal to nothing later.
///
/// `line_number` is the section's own `[header]` line — reporting the *next*
/// header's line, as this used to, points past the block that is actually
/// wrong.
fn finish(record: ShaderRecord, line_number: usize) -> Result<ShaderRecord, String> {
    for (field, value) in [
        ("source", &record.source),
        ("source-sha256", &record.source_sha256),
        ("spirv", &record.spirv),
        ("spirv-sha256", &record.spirv_sha256),
    ] {
        if value.is_empty() {
            return Err(format!(
                "line {line_number}: section [{}] has no `{field}`",
                record.name
            ));
        }
    }
    // Each optional artifact's path and hash are optional *together*: a path
    // with no hash used to reach build.rs and fail its comparison against `""`,
    // with a message naming drift rather than the missing key.
    for (path_key, path, hash_key, hash) in [
        ("wgsl", &record.wgsl, "wgsl-sha256", &record.wgsl_sha256),
        ("msl", &record.msl, "msl-sha256", &record.msl_sha256),
    ] {
        if path.is_empty() == hash.is_empty() {
            continue;
        }
        let (present, missing) = if path.is_empty() {
            (hash_key, path_key)
        } else {
            (path_key, hash_key)
        };
        return Err(format!(
            "line {line_number}: section [{}] has `{present}` but no `{missing}`; both or neither",
            record.name
        ));
    }
    if record.entry_points.is_empty() {
        return Err(format!(
            "line {line_number}: section [{}] declares no entry points, so nothing could use it",
            record.name
        ));
    }
    // A DXIL container is addressed by the entry point it holds, so one naming
    // an entry point the shader does not declare is a container nothing can
    // ever ask for — and, far more likely, a generator that renamed an entry
    // point in one place and not the other.
    for artifact in &record.dxil {
        if !record
            .entry_points
            .iter()
            .any(|(name, _)| *name == artifact.entry_point)
        {
            return Err(format!(
                "line {line_number}: section [{}] has a DXIL artifact for `{}`, which is not one \
                 of its entry points",
                record.name, artifact.entry_point
            ));
        }
    }
    Ok(record)
}

/// A 64-character lower-case hex digest, or an explanation.
fn hex(value: &str, line_number: usize) -> Result<String, String> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!(
            "line {line_number}: {value:?} is not a 64-character hex SHA-256"
        ));
    }
    Ok(value.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# a comment
slangc-version = 2026.14
target = spirv_1_5

[triangle]
source = shaders/triangle.slang
source-sha256 = e1a5e7acd7da40b0f27f685827467a36dd49eb8ba5978a3dfd03f36d8a33af66
spirv = spirv/triangle.spv
spirv-sha256 = d1a318ebaf3e333a2b7646473aae647d55011b3e91036540beb491896bea6d33
entry-points = vertexMain:vertex, fragmentMain:fragment
";

    /// The last header key of [`SAMPLE`], and so where another one is spliced
    /// in: a header key after the first `[section]` is an unknown key by
    /// design, which would make a DXIL test fail for the wrong reason.
    const HEADER_END: &str = "target = spirv_1_5";

    /// A digest that is syntactically a SHA-256 and is nothing's.
    const ZERO: &str = "0000000000000000000000000000000000000000000000000000000000000000";

    /// [`SAMPLE`] with the DXIL compiler pins in its header.
    fn pinned() -> String {
        SAMPLE.replace(
            HEADER_END,
            &format!("{HEADER_END}\ndxc-version = 1.9\ndxil-model = 6_6"),
        )
    }

    #[test]
    fn a_well_formed_manifest_round_trips_into_records() {
        let manifest = parse_manifest(SAMPLE).expect("parses");
        assert_eq!(manifest.slangc_version, "2026.14");
        assert_eq!(manifest.target, "spirv_1_5");
        assert_eq!(manifest.shaders.len(), 1);

        let record = &manifest.shaders[0];
        assert_eq!(record.name, "triangle");
        assert_eq!(record.source, "shaders/triangle.slang");
        assert_eq!(record.spirv, "spirv/triangle.spv");
        assert_eq!(
            record.entry_points,
            vec![
                ("vertexMain".to_string(), "vertex".to_string()),
                ("fragmentMain".to_string(), "fragment".to_string()),
            ],
            "entry point case must survive verbatim — it is matched against \
             OpEntryPoint"
        );
    }

    /// The failure this parser exists to prevent: a record that is silently
    /// dropped or silently half-filled turns the drift check into a no-op.
    #[test]
    fn an_incomplete_section_is_refused_rather_than_half_parsed() {
        let text = "slangc-version = 1\n[triangle]\nsource = a.slang\n";
        let error = parse_manifest(text).expect_err("a section with no hashes is unusable");
        assert!(error.contains("source-sha256"), "{error}");

        let no_entries = "slangc-version = 1\n[t]\nsource = a\nsource-sha256 = \
                          0000000000000000000000000000000000000000000000000000000000000000\n\
                          spirv = b\nspirv-sha256 = \
                          0000000000000000000000000000000000000000000000000000000000000000\n";
        let error = parse_manifest(no_entries).expect_err("no entry points is unusable");
        assert!(error.contains("entry points"), "{error}");
    }

    /// A truncated or mistyped digest must be rejected at parse time. An empty
    /// string compared against a real hash would simply never match, which
    /// looks like drift; a *short* one is the case that could silently compare
    /// equal to a prefix if anyone ever loosened the comparison.
    #[test]
    fn a_digest_that_is_not_a_sha256_is_refused() {
        let text = "slangc-version = 1\n[t]\nsource-sha256 = deadbeef\n";
        let error = parse_manifest(text).expect_err("eight hex characters is not a SHA-256");
        assert!(error.contains("hex"), "{error}");
        assert!(
            error.contains("line 3"),
            "the line number must be usable: {error}"
        );
    }

    #[test]
    fn a_manifest_with_no_shaders_or_no_version_is_refused() {
        assert!(parse_manifest("slangc-version = 1\n").is_err());
        assert!(parse_manifest("[t]\n").is_err());
        assert!(parse_manifest("").is_err());
    }

    #[test]
    fn unknown_keys_are_errors_rather_than_ignored() {
        let text = format!("{SAMPLE}mystery = value\n");
        let error = parse_manifest(&text).expect_err("a key nobody reads is a silent hole");
        assert!(error.contains("unknown key"), "{error}");
    }

    #[test]
    fn comments_and_blank_lines_are_ignored_anywhere() {
        let text = SAMPLE.replace("[triangle]", "# noise\n\n[triangle]   # trailing\n");
        let manifest = parse_manifest(&text).expect("parses");
        assert_eq!(manifest.shaders.len(), 1);
        assert_eq!(manifest.shaders[0].name, "triangle");
    }

    /// `build.rs` turns the section name into a `pub static` identifier, so a
    /// name that is not one must fail here — naming the manifest line —
    /// rather than as a syntax error inside generated code.
    #[test]
    fn a_section_name_that_is_not_an_identifier_is_refused() {
        for bad in ["2d blur", "2dblur", "blur.pass", "post fx"] {
            let text = format!("slangc-version = 1\n[{bad}]\n");
            let error = parse_manifest(&text).expect_err("{bad} is not an identifier");
            assert!(error.contains("line 2"), "{bad}: {error}");
        }
    }

    /// Two sections with the same name produced two records, and therefore two
    /// identically-named `pub static`s.
    #[test]
    fn a_duplicate_section_name_is_refused() {
        let text = format!("{SAMPLE}\n[triangle]\nsource = other.slang\n");
        let error = parse_manifest(&text).expect_err("two [triangle]s collide");
        assert!(error.contains("twice"), "{error}");
    }

    /// A repeated key used to last-wins silently, so a manifest with two
    /// `spirv-sha256` lines checked only one of them.
    #[test]
    fn a_repeated_key_is_refused() {
        let text = format!("{SAMPLE}source = shaders/other.slang\n");
        let error = parse_manifest(&text).expect_err("`source` twice is ambiguous");
        assert!(error.contains("twice"), "{error}");
    }

    /// Each optional artifact's path and hash are optional *together*. One
    /// without the other used to reach `build.rs` and fail there against `""`,
    /// with a message naming drift rather than the missing key.
    ///
    /// Both optional columns are checked, because the pairing rule is one loop
    /// now and a loop that only ever ran over `wgsl` would pass identically.
    #[test]
    fn an_artifact_path_without_its_hash_is_refused() {
        for (path_key, path, hash_key) in [
            ("wgsl", "wgsl/triangle.wgsl", "wgsl-sha256"),
            ("msl", "msl/triangle.metal", "msl-sha256"),
        ] {
            let text = format!("{SAMPLE}{path_key} = {path}\n");
            let error = parse_manifest(&text).expect_err("a path with no hash is unusable");
            assert!(error.contains(hash_key), "{path_key}: {error}");

            let text = format!(
                "{SAMPLE}{hash_key} = \
                 0000000000000000000000000000000000000000000000000000000000000000\n"
            );
            let error = parse_manifest(&text).expect_err("a hash with no path is unusable");
            assert!(
                error.contains(&format!("`{path_key}`")),
                "{path_key}: {error}"
            );
        }
    }

    /// The error must point at the offending section's own header, not at the
    /// next one.
    #[test]
    fn an_incomplete_section_names_its_own_header_line() {
        let text = "slangc-version = 1\n\
                    [first]\n\
                    source = a.slang\n\
                    \n\
                    [second]\n\
                    source = b.slang\n";
        let error = parse_manifest(text).expect_err("[first] has no hashes");
        assert!(
            error.contains("line 2") && error.contains("[first]"),
            "the error should name [first]'s header on line 2: {error}"
        );
    }

    /// A `#` inside a value is not a comment — only one at the start of a line
    /// or after whitespace is.
    #[test]
    fn a_hash_inside_a_value_is_not_a_comment() {
        assert_eq!(strip_comment("source = a#b.slang"), "source = a#b.slang");
        assert_eq!(
            strip_comment("source = a.slang # note"),
            "source = a.slang "
        );
        assert_eq!(strip_comment("# whole line"), "");
    }

    /// The real manifest must parse, or the build script's error would be the
    /// first anyone hears of a formatting change in the generator script.
    #[test]
    fn the_committed_manifest_parses() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("spirv/manifest.txt");
        let text = std::fs::read_to_string(&path).expect("the manifest is committed");
        let manifest = parse_manifest(&text).expect("the committed manifest parses");
        assert!(!manifest.shaders.is_empty());
        assert!(!manifest.slangc_version.is_empty());
        // Every optional column is filled for every shipped shader. The
        // pairing rule in `finish` only checks path-and-hash *together*, so a
        // whole column silently missing from the generator would parse fine.
        for record in &manifest.shaders {
            assert!(!record.wgsl.is_empty(), "{}: no wgsl column", record.name);
            assert!(!record.msl.is_empty(), "{}: no msl column", record.name);
            assert!(!record.msl_sha256.is_empty(), "{}", record.name);
            // One DXIL container per entry point, not one per shader — so the
            // count is the check. A generator that emitted only the first entry
            // point would leave every fragment stage unusable on D3D12, and a
            // "there is a dxil column" assertion would pass.
            assert_eq!(
                record.dxil.len(),
                record.entry_points.len(),
                "{}: {} entry points but {} DXIL containers",
                record.name,
                record.entry_points.len(),
                record.dxil.len()
            );
        }
        assert!(!manifest.dxc_version.is_empty());
        assert_eq!(manifest.dxil_model, "6_6");
    }

    /// A DXIL line carries the entry point, the path and the digest together,
    /// so the three cannot drift — and each malformed shape is refused rather
    /// than half-read.
    #[test]
    fn a_malformed_dxil_line_is_refused() {
        let bad = [
            // Two fields rather than three: no digest at all.
            "dxil = vertexMain:dxil/triangle.vertexMain.dxil",
            // Four: a path with a colon in it, which would otherwise silently
            // truncate.
            "dxil = vertexMain:dxil/a:b.dxil:\
             0000000000000000000000000000000000000000000000000000000000000000",
            // A digest that is not one.
            "dxil = vertexMain:dxil/triangle.vertexMain.dxil:deadbeef",
            // An empty entry point.
            "dxil = :dxil/triangle.vertexMain.dxil:\
             0000000000000000000000000000000000000000000000000000000000000000",
        ];
        for line in bad {
            let text = format!("{SAMPLE}{line}\n");
            parse_manifest(&text).expect_err(line);
        }
    }

    /// A container for an entry point the shader does not declare is one
    /// nothing can ever ask for, because a DXIL artifact is addressed by name.
    #[test]
    fn a_dxil_artifact_for_an_undeclared_entry_point_is_refused() {
        let text = format!(
            "{SAMPLE}dxil = computeMain:dxil/triangle.computeMain.dxil:\
             0000000000000000000000000000000000000000000000000000000000000000\n"
        );
        let error = parse_manifest(&text).expect_err("triangle declares no computeMain");
        assert!(error.contains("computeMain"), "{error}");

        // And the same line for an entry point it *does* declare parses, so the
        // refusal above is the name check rather than the shape.
        let text = format!(
            "{}dxil = vertexMain:dxil/triangle.vertexMain.dxil:\
             0000000000000000000000000000000000000000000000000000000000000000\n",
            pinned()
        );
        let manifest = parse_manifest(&text).expect("a declared entry point parses");
        assert_eq!(manifest.shaders[0].dxil.len(), 1);
        assert_eq!(manifest.shaders[0].dxil[0].entry_point, "vertexMain");
    }

    /// Two containers claiming one entry point means one of them is dead, and
    /// which one is dead would depend on lookup order.
    #[test]
    fn two_dxil_artifacts_for_one_entry_point_are_refused() {
        let text = format!(
            "{}dxil = vertexMain:dxil/a.dxil:{ZERO}\ndxil = vertexMain:dxil/b.dxil:{ZERO}\n",
            pinned()
        );
        let error = parse_manifest(&text).expect_err("one entry point, two containers");
        assert!(error.contains("unreachable"), "{error}");
    }

    /// DXIL with no compiler pin is an artifact nothing can re-derive, which is
    /// the whole basis of the drift check.
    #[test]
    fn dxil_without_a_compiler_pin_is_refused() {
        let text = format!("{SAMPLE}dxil = vertexMain:dxil/a.dxil:{ZERO}\n");
        let error = parse_manifest(&text).expect_err("no dxc-version");
        assert!(error.contains("dxc-version"), "{error}");

        let text = format!(
            "{}dxil = vertexMain:dxil/a.dxil:{ZERO}\n",
            SAMPLE.replace(HEADER_END, &format!("{HEADER_END}\ndxc-version = 1.9"))
        );
        let error = parse_manifest(&text).expect_err("no dxil-model");
        assert!(error.contains("dxil-model"), "{error}");
    }
}
