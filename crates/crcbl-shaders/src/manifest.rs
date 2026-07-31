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
    let mut current: Option<ShaderRecord> = None;

    for (index, raw) in text.lines().enumerate() {
        let line_number = index + 1;
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }

        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            if let Some(record) = current.take() {
                manifest.shaders.push(finish(record, line_number)?);
            }
            let name = name.trim();
            if name.is_empty() {
                return Err(format!("line {line_number}: empty section name"));
            }
            current = Some(ShaderRecord {
                name: name.to_string(),
                source: String::new(),
                source_sha256: String::new(),
                spirv: String::new(),
                spirv_sha256: String::new(),
                wgsl: String::new(),
                wgsl_sha256: String::new(),
                entry_points: Vec::new(),
            });
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            return Err(format!(
                "line {line_number}: expected `key = value`, got {line:?}"
            ));
        };
        let (key, value) = (key.trim(), value.trim());

        match (&mut current, key) {
            (None, "slangc-version") => manifest.slangc_version = value.to_string(),
            (None, "target") => manifest.target = value.to_string(),
            (None, other) => {
                return Err(format!(
                    "line {line_number}: `{other}` appears before any [section], and is not a \
                     header key"
                ));
            }
            (Some(record), "source") => record.source = value.to_string(),
            (Some(record), "source-sha256") => record.source_sha256 = hex(value, line_number)?,
            (Some(record), "spirv") => record.spirv = value.to_string(),
            (Some(record), "spirv-sha256") => record.spirv_sha256 = hex(value, line_number)?,
            (Some(record), "wgsl") => record.wgsl = value.to_string(),
            (Some(record), "wgsl-sha256") => record.wgsl_sha256 = hex(value, line_number)?,
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

    if let Some(record) = current.take() {
        let last = text.lines().count();
        manifest.shaders.push(finish(record, last)?);
    }
    if manifest.slangc_version.is_empty() {
        return Err("the manifest names no `slangc-version`".to_string());
    }
    if manifest.shaders.is_empty() {
        return Err("the manifest describes no shaders".to_string());
    }
    Ok(manifest)
}

/// Rejects a section that is missing a field, rather than letting an empty
/// hash compare equal to nothing later.
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
    if record.entry_points.is_empty() {
        return Err(format!(
            "line {line_number}: section [{}] declares no entry points, so nothing could use it",
            record.name
        ));
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

    /// The real manifest must parse, or the build script's error would be the
    /// first anyone hears of a formatting change in the generator script.
    #[test]
    fn the_committed_manifest_parses() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("spirv/manifest.txt");
        let text = std::fs::read_to_string(&path).expect("the manifest is committed");
        let manifest = parse_manifest(&text).expect("the committed manifest parses");
        assert!(!manifest.shaders.is_empty());
        assert!(!manifest.slangc_version.is_empty());
    }
}
