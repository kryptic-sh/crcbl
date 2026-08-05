//! The workgroup size and uniform block `compute_probe.slang` declares, in the
//! layouts that shader declares.
//!
//! Same reason as [`crate::triangle`]: the shader fixes a number and a byte
//! layout, every producer of those has to agree with it exactly, and keeping
//! both in the crate that owns the source means there is one place to change
//! rather than one per consumer. The workgroup size is the one that bites — a
//! caller computing `groups = elements / 32` against a `[numthreads(64, …)]`
//! shader dispatches half the work and every element past the halfway point
//! keeps whatever was in the buffer before.

/// Invocations per workgroup, matching `[numthreads(64, 1, 1)]` in
/// `shaders/compute_probe.slang`.
///
/// A caller dispatches `elements.div_ceil(WORKGROUP_SIZE)` groups; the shader
/// discards the invocations past [`Params::count`] that the last group brings.
pub const WORKGROUP_SIZE: u32 = 64;

/// Bytes of the uniform block: one `uint`, padded to the 16-byte row `std140`
/// gives a uniform buffer.
pub const PARAMS_SIZE: usize = 16;

/// The uniform block, matching `struct Params` in
/// `shaders/compute_probe.slang`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Params {
    /// Elements in the source and destination buffers. Invocations at or past
    /// this index write nothing.
    pub count: u32,
}

impl Params {
    /// The block as the bytes a uniform buffer holds.
    ///
    /// Little-endian, then zero padding out to [`PARAMS_SIZE`]. The padding is
    /// written rather than left alone because a uniform buffer allocated for
    /// this block is `PARAMS_SIZE` bytes wide and a partial write would leave
    /// the tail undefined — harmless while the block has one member, and
    /// exactly the sort of thing that stops being harmless when it gains a
    /// second.
    #[must_use]
    pub fn to_bytes(self) -> [u8; PARAMS_SIZE] {
        let mut bytes = [0u8; PARAMS_SIZE];
        bytes[0..4].copy_from_slice(&self.count.to_le_bytes());
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The constant and the shader must name the same workgroup size.
    ///
    /// Nothing else can catch this: the shader compiles, the dispatch succeeds,
    /// and a mismatch shows up only as a partially-written buffer on whatever
    /// machine happens to look. Reading the source is the check, and the source
    /// is hash-pinned by the manifest, so it is the same file the committed
    /// artifact was built from.
    #[test]
    fn the_workgroup_size_matches_the_numthreads_the_shader_declares() {
        let source = include_str!("../shaders/compute_probe.slang");
        let declaration = format!("[numthreads({WORKGROUP_SIZE}, 1, 1)]");
        assert!(
            source.contains(&declaration),
            "compute_probe.slang does not declare `{declaration}`; WORKGROUP_SIZE has drifted \
             from the shader"
        );
    }

    /// The padding claim, checked rather than asserted in prose.
    #[test]
    fn the_uniform_block_is_one_le_u32_in_a_sixteen_byte_row() {
        let bytes = Params { count: 0x0102_0304 }.to_bytes();
        assert_eq!(bytes.len(), PARAMS_SIZE);
        assert_eq!(&bytes[0..4], &[0x04, 0x03, 0x02, 0x01]);
        assert!(bytes[4..].iter().all(|byte| *byte == 0), "{bytes:?}");
    }
}
