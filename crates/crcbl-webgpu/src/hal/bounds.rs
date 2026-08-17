//! The stream's field caps, measured where a caller can still be told.
//!
//! [`tag::MAX_FIELD_BYTES`] and [`tag::MAX_ELEMENT_COUNT`] are the caps the
//! reader enforces, and the writer asserts against them so nothing this crate
//! encodes is something it would refuse to decode. An assert is the right
//! backstop for an invariant and the wrong answer for caller data: on wasm a
//! `panic!` compiles to `unreachable`, so the whole module dies mid-frame and
//! the page is left holding a trap where a diagnosis should be — which is
//! exactly how a scene whose vertex upload was larger than a field took the
//! render harness down without naming itself.
//!
//! So every caller-supplied field whose length is *data* is measured here,
//! before a byte of the command is written, and one past a cap comes back as a
//! [`HalError`] the caller can report — the shape
//! [`HalError::Unsupported`](crcbl_hal::HalError::Unsupported) already gives the
//! ops this backend has no command for.
//!
//! # What is deliberately not measured
//!
//! Fields whose length the program that wrote them fixes: a debug label, an
//! entry point name, an attachment or binding list the device's own limits
//! bound. Those keep the writer's assert, which is what an invariant no input
//! reaches should have.
//!
//! [`Device::write_buffer`](crcbl_hal::Device::write_buffer) is absent for a
//! third reason — its upload has no cap left to check.
//! [`StreamWriter::write_buffer`](crate::StreamWriter::write_buffer) splits one
//! past [`tag::MAX_FIELD_BYTES`] across as many commands as it takes.

use crcbl_hal::{HalError, ShaderModuleDesc};

use crate::tag;

/// Refuse a caller-supplied byte field longer than the stream can carry.
pub(super) fn field(what: &str, len: usize) -> Result<(), HalError> {
    if len > tag::MAX_FIELD_BYTES {
        return Err(HalError::InvalidDescriptor(format!(
            "{what} is {len} bytes, past the {} a single WebGPU stream field carries",
            tag::MAX_FIELD_BYTES
        )));
    }
    Ok(())
}

/// Refuse a caller-supplied array longer than the stream can carry.
pub(super) fn count(what: &str, len: usize) -> Result<(), HalError> {
    if len > tag::MAX_ELEMENT_COUNT {
        return Err(HalError::InvalidDescriptor(format!(
            "{what} holds {len} elements, past the {} a WebGPU stream array carries",
            tag::MAX_ELEMENT_COUNT
        )));
    }
    Ok(())
}

/// Measure a shader module's four artifacts.
///
/// The one descriptor on this seam carrying compiled output rather than program
/// text: SPIR-V comes out of a build pipeline and WGSL out of an asset, so their
/// sizes are the shader's business and not the call site's. `spirv` is measured
/// as a *word* count because that is how it crosses — [`tag::MAX_ELEMENT_COUNT`]
/// words, not bytes.
pub(super) fn shader_module(desc: &ShaderModuleDesc<'_>) -> Result<(), HalError> {
    count("ShaderModuleDesc::spirv", desc.spirv.len())?;
    field("ShaderModuleDesc::wgsl", desc.wgsl.map_or(0, str::len))?;
    field("ShaderModuleDesc::msl", desc.msl.map_or(0, str::len))?;
    count("ShaderModuleDesc::dxil", desc.dxil.len())?;
    for (entry_point, container) in desc.dxil {
        field(
            &format!("ShaderModuleDesc::dxil entry point {entry_point:?}"),
            entry_point.len(),
        )?;
        field(
            &format!("ShaderModuleDesc::dxil container for {entry_point:?}"),
            container.len(),
        )?;
    }
    Ok(())
}
