//! `write-triangle-glb` — put `crcbl_scene::gltf_fixture`'s triangle on disk.
//!
//! ```text
//! cargo run -p crcbl-scene --features gltf-fixture \
//!     --example write-triangle-glb -- <out.glb>
//! ```
//!
//! One argument, one file, and nothing else touched. It exists so that a
//! harness which has to point a tool at a real model has one to point at —
//! `tools/run-samples-windowed.sh` runs `viewer` against what this writes —
//! without a `.glb` being committed. The [module docs] make that argument in
//! full; this is only the half that reaches a filesystem.
//!
//! # It reads back what it just wrote
//!
//! The bytes go out and then come *in* again, through
//! [`crcbl_scene::gltf_import::import_gltf`] over a real
//! [`DirSource`](crcbl_assets::DirSource) rooted at the file's own directory —
//! the same route the tool that opens it will take. So a fixture that stopped
//! being valid glTF fails here, naming the document, instead of failing a
//! window harness three steps later with a message about a model the harness
//! did not write.
//!
//! A run that fails that check removes the file. Leaving it would mean a broken
//! document sitting where the next thing to look expects a good one, and the
//! only file removed is the one this process created a moment earlier.
//!
//! Exit codes match the samples': **0** it wrote and re-read the document,
//! **1** it could not, **2** the arguments were wrong — a path already taken
//! included, since overwriting is refused rather than done.
//!
//! [module docs]: crcbl_scene::gltf_fixture

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::Path;
use std::process::ExitCode;

use crcbl_assets::DirSource;
use crcbl_scene::gltf_fixture::{BIN_CHUNK_BUFFER, glb, triangle_bin, triangle_json};
use crcbl_scene::gltf_import::import_gltf;

const USAGE: &str = "\
write-triangle-glb — write crcbl-scene's triangle fixture to a .glb

USAGE:
    write-triangle-glb <OUT.glb>

ARGUMENTS:
    <OUT.glb>   Where to write it. The file must not already exist — this
                overwrites nothing — and its directory must, since nothing
                here creates one. The name becomes an asset key when the
                document is read back, so keep it to letters, digits, '.',
                '_' and '-'.

OPTIONS:
    -h, --help  Print this text.";

/// Why the document did not land, and which exit code says so.
enum Failure {
    /// The invocation named a path that cannot be written to. Nothing was
    /// written; exit 2, as for any other bad argument.
    Usage(String),
    /// The bytes reached the disk and did not come back. Exit 1.
    Rejected(String),
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = match args.as_slice() {
        [flag] if flag == "-h" || flag == "--help" => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        [out] => Path::new(out),
        _ => {
            eprintln!("write-triangle-glb: one output path, and nothing else\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    match write(out) {
        Ok(report) => {
            println!("write-triangle-glb: wrote {} — {report}", out.display());
            ExitCode::SUCCESS
        }
        Err(Failure::Usage(message)) => {
            eprintln!("write-triangle-glb: {message}\n\n{USAGE}");
            ExitCode::from(2)
        }
        Err(Failure::Rejected(message)) => {
            eprintln!("write-triangle-glb: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Writes the fixture to `out` and reads it back, answering what came back.
fn write(out: &Path) -> Result<String, Failure> {
    let Some(name) = out.file_name() else {
        return Err(Failure::Usage(format!(
            "`{}` does not name a file",
            out.display()
        )));
    };
    // The asset root the document will be read through, here and in whatever
    // opens it afterwards. An `out` of `model.glb` has an empty parent, which
    // is not the same directory as `.` to `DirSource`.
    let root = match out.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };

    let bytes = glb(&triangle_json(BIN_CHUNK_BUFFER), Some(&triangle_bin()));

    // `create_new` rather than "check, then write": one syscall, so there is no
    // window in which a file appears between the two, and a path that is
    // already taken is left exactly as it was.
    let mut file = match fs::File::create_new(out) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            return Err(Failure::Usage(format!(
                "`{}` already exists; nothing was written — remove it or name another path",
                out.display()
            )));
        }
        Err(error) => {
            return Err(Failure::Usage(format!(
                "`{}` could not be created: {error}",
                out.display()
            )));
        }
    };
    if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
        return Err(Failure::Rejected(format!(
            "`{}` could not be written: {error}",
            out.display()
        )));
    }
    drop(file);

    let source = DirSource::at(root.to_path_buf());
    match import_gltf(&source, Path::new(name)) {
        Ok(scene) => Ok(format!(
            "{} bytes, {} mesh(es), {} instance(s)",
            bytes.len(),
            scene.meshes().len(),
            scene.instances().len()
        )),
        Err(error) => {
            let removed = match fs::remove_file(out) {
                Ok(()) => "removed".to_string(),
                Err(error) => format!("and could not be removed either: {error}"),
            };
            Err(Failure::Rejected(format!(
                "`{}` is not a document this engine can read back: {error} ({removed})",
                out.display()
            )))
        }
    }
}
