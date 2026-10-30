//! Driver bugs this backend works around by reporting less than the driver
//! claims.
//!
//! Each workaround is a pure predicate over what the driver says about itself,
//! so it can be tested against the exact strings real drivers report, and each
//! has an environment variable that turns it off again — the way to check
//! whether a newer driver has fixed the bug without rebuilding.
//!
//! # Mesh shading on radv RDNA2
//!
//! radv lists `VK_EXT_mesh_shader` with `meshShader` and `taskShader` on RDNA2
//! (GFX10.3), and the renderer's mesh path loses the device there: on the
//! Ryzen 9 9950X3D's integrated Radeon (`RADV RAPHAEL_MENDOCINO`, Mesa 26.2.6)
//! `quarry --force-geometry mesh-shader` hangs the compute ring within two
//! frames (`ring comp_1.2.0 timeout`, "context is guilty of a hard recovery",
//! `VK_ERROR_DEVICE_LOST`), while the same scene on the indirect-count path
//! runs clean and validation plus synchronisation validation report nothing.
//! Both GPUs on that machine report identical
//! `VkPhysicalDeviceMeshShaderPropertiesEXT`, and the RDNA3 one
//! (`RADV NAVI31`, same Mesa) runs the mesh path clean, so the engine is not
//! exceeding a limit only one of them has. Withholding only the task stage is
//! not enough: the mesh path without it hung the same iGPU's gfx ring instead.
//! `adapter::describe` therefore withholds both
//! [`Features::MESH_SHADER`](crcbl_hal::Features::MESH_SHADER) and
//! [`Features::TASK_SHADER`](crcbl_hal::Features::TASK_SHADER) on a device
//! [`radv_rdna2_family`] names, so geometry-path selection falls back to
//! indirect count; [`MESH_ON_RDNA2_VAR`] set to `1` reports them again.

use ash::vk;

/// The variable that reports mesh shading on radv RDNA2 again, with `1`: for
/// checking whether a driver still hangs, not for shipping.
pub(crate) const MESH_ON_RDNA2_VAR: &str = "CRCBL_VK_MESH_ON_RDNA2";

/// The family names radv puts in its device name for RDNA2 (GFX10.3) chips.
///
/// radv names a device `<marketing name> (RADV <family><compiler>)`, or
/// `AMD RADV <family><compiler>` where it has no marketing name — both format
/// strings are in Mesa 26.2.6's `libvulkan_radeon.so` — the family being
/// Mesa's `ac_get_family_name` and the compiler empty for ACO or ` (LLVM
/// x.y.z)` for LLVM. Each name below was read out of Mesa 26.2.6's
/// `libvulkan_radeon.so`: the discrete Navi 2x parts, the Steam Deck's Van
/// Gogh, Rembrandt (Ryzen 6000 mobile) and Raphael/Mendocino (the Ryzen
/// 7000/9000 desktop iGPU and Mendocino mobile). GFX10.1's `NAVI10`/`NAVI12`/
/// `NAVI14`/`GFX1013` are left out: Mesa enables `VK_EXT_mesh_shader` from
/// GFX10.3 on, which was not re-checked against this Mesa's sources.
const RADV_RDNA2_FAMILIES: &[&str] = &[
    "NAVI21",
    "NAVI22",
    "NAVI23",
    "NAVI24",
    "VANGOGH",
    "REMBRANDT",
    "RAPHAEL_MENDOCINO",
];

/// The RDNA2 family `device_name` names when `driver_id` is radv, or `None`.
///
/// Verified on hardware only on `RAPHAEL_MENDOCINO` (where the mesh path
/// hangs) and `NAVI31` (RDNA3, where it runs, and which this does not match).
/// The other families are matched because they are the same GFX10.3
/// generation under the same driver; the Steam Deck's `VANGOGH` is the one
/// that matters most and has not been run. Nothing but radv is matched: the
/// hang was only ever seen under radv, and AMD's own drivers were not tried.
#[must_use]
pub(crate) fn radv_rdna2_family(
    driver_id: vk::DriverId,
    device_name: &str,
) -> Option<&'static str> {
    if driver_id != vk::DriverId::MESA_RADV {
        return None;
    }
    // The family is the identifier right after `RADV `; the compiler suffix
    // after it is not part of it.
    let (_, after) = device_name.split_once("RADV ")?;
    let end = after
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(after.len());
    let family = &after[..end];
    RADV_RDNA2_FAMILIES
        .iter()
        .copied()
        .find(|&known| known == family)
}

/// Whether [`MESH_ON_RDNA2_VAR`] asks for mesh shading back, read once per
/// adapter description.
#[must_use]
pub(crate) fn mesh_on_rdna2_wanted() -> bool {
    wanted_by(std::env::var(MESH_ON_RDNA2_VAR).ok().as_deref())
}

/// [`mesh_on_rdna2_wanted`] with the variable's value given: `1` and nothing
/// else, as `CRCBL_VK_SHADER_STATS` reads.
fn wanted_by(value: Option<&str>) -> bool {
    value.map(str::trim) == Some("1")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The names radv reported on the machine the hang was found on, verbatim
    /// from `vulkaninfo`.
    const RAPHAEL: &str = "AMD Ryzen 9 9950X3D 16-Core Processor (RADV RAPHAEL_MENDOCINO)";
    const NAVI31: &str = "AMD Radeon RX 7900 XTX (RADV NAVI31)";

    #[test]
    fn radv_rdna2_is_matched_by_family() {
        assert_eq!(
            radv_rdna2_family(vk::DriverId::MESA_RADV, RAPHAEL),
            Some("RAPHAEL_MENDOCINO")
        );
        // Spelled out rather than read from the list, so a family dropped from
        // it fails here.
        for family in [
            "NAVI21",
            "NAVI22",
            "NAVI23",
            "NAVI24",
            "VANGOGH",
            "REMBRANDT",
            "RAPHAEL_MENDOCINO",
        ] {
            let name = format!("AMD Custom GPU 0405 (RADV {family})");
            assert_eq!(
                radv_rdna2_family(vk::DriverId::MESA_RADV, &name),
                Some(family),
                "{name}"
            );
        }
        // Both of radv's formats, with the suffix an LLVM-compiled radv
        // appends; the family still reads.
        for name in [
            "AMD Radeon RX 6800 (RADV NAVI21 (LLVM 23.1.4))",
            "AMD RADV VANGOGH",
            "AMD RADV VANGOGH (LLVM 23.1.4)",
        ] {
            assert!(
                radv_rdna2_family(vk::DriverId::MESA_RADV, name).is_some(),
                "{name}"
            );
        }
    }

    /// RDNA3 and GFX10.1 under radv, and a family name sharing a prefix with
    /// one on the list, keep mesh shading.
    #[test]
    fn other_radv_families_are_not_matched() {
        assert_eq!(radv_rdna2_family(vk::DriverId::MESA_RADV, NAVI31), None);
        for name in [
            "AMD Radeon RX 7600 (RADV NAVI33)",
            "AMD Radeon 780M (RADV PHOENIX)",
            "AMD Radeon RX 5700 XT (RADV NAVI10)",
            "AMD Radeon RX 9070 XT (RADV GFX1201)",
            "AMD Radeon (RADV NAVI210)",
            "AMD Radeon (RADV RAPHAEL)",
            "AMD Radeon RX 6800 (no family)",
        ] {
            assert_eq!(
                radv_rdna2_family(vk::DriverId::MESA_RADV, name),
                None,
                "{name}"
            );
        }
    }

    /// The name alone never decides: the same family string under another
    /// driver is not radv's bug.
    #[test]
    fn only_radv_is_matched() {
        for driver in [
            vk::DriverId::AMD_PROPRIETARY,
            vk::DriverId::AMD_OPEN_SOURCE,
            vk::DriverId::MESA_LLVMPIPE,
        ] {
            assert_eq!(radv_rdna2_family(driver, RAPHAEL), None, "{driver:?}");
        }
    }

    #[test]
    fn the_override_wants_exactly_one() {
        assert!(wanted_by(Some("1")));
        assert!(wanted_by(Some(" 1\n")));
        for value in [None, Some(""), Some("0"), Some("yes"), Some("true")] {
            assert!(!wanted_by(value), "{value:?}");
        }
    }
}
