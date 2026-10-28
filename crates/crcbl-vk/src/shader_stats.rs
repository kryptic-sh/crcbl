//! What a driver compiled each pipeline into, logged when asked for.
//!
//! `CRCBL_VK_SHADER_STATS=1` makes a device enable
//! `VK_KHR_pipeline_executable_properties` where the driver offers it, create
//! every pipeline with `VK_PIPELINE_CREATE_CAPTURE_STATISTICS_BIT_KHR`, and log
//! each executable's statistics at info: one line per stage, with the counts
//! the driver chooses to report (registers, scratch, instructions and the
//! like, under the driver's own names). It is a profiling aid for shader cost
//! across vendors, the cross-driver form of Mesa's `INTEL_DEBUG=fs` or
//! `RADV_DEBUG=shaderstats`; an ordinary device is created exactly as without
//! it.

use ash::{khr, vk};

/// The variable that turns the statistics on, with `1`.
pub(crate) const VAR: &str = "CRCBL_VK_SHADER_STATS";

/// Whether [`VAR`] asks for the statistics, read once per device open.
pub(crate) fn wanted() -> bool {
    wanted_by(std::env::var(VAR).ok().as_deref())
}

/// [`wanted`] with the variable's value given: `1` and nothing else.
fn wanted_by(value: Option<&str>) -> bool {
    value.map(str::trim) == Some("1")
}

/// Whether `physical` lists the extension and grants its feature.
pub(crate) fn supported(instance: &ash::Instance, physical: vk::PhysicalDevice) -> bool {
    // SAFETY: `physical` came from `instance`; the call reads driver tables.
    let listed = unsafe { instance.enumerate_device_extension_properties(physical) }
        .unwrap_or_default()
        .iter()
        .any(|extension| {
            extension.extension_name_as_c_str() == Ok(khr::pipeline_executable_properties::NAME)
        });
    if !listed {
        return false;
    }
    let mut feature = vk::PhysicalDevicePipelineExecutablePropertiesFeaturesKHR::default();
    let mut features2 = vk::PhysicalDeviceFeatures2::default().push_next(&mut feature);
    // SAFETY: as above; the chained struct is the extension's own, which the
    // driver listed.
    unsafe { instance.get_physical_device_features2(physical, &mut features2) };
    feature.pipeline_executable_info == vk::TRUE
}

/// Logs every executable of `pipeline`, labelled `label`.
///
/// A failed query is logged and skipped: these are diagnostics, and a
/// pipeline that compiled is not to be refused for want of them.
pub(crate) fn log(
    ext: &khr::pipeline_executable_properties::Device,
    pipeline: vk::Pipeline,
    label: Option<&str>,
) {
    let label = label.unwrap_or("<unlabelled>");
    let info = vk::PipelineInfoKHR::default().pipeline(pipeline);
    // SAFETY: `pipeline` is a live pipeline of the device `ext` was loaded
    // for, created with the capture flag.
    let executables = match unsafe { ext.get_pipeline_executable_properties(&info) } {
        Ok(executables) => executables,
        Err(error) => {
            crcbl_core::log::warn!("crcbl-vk: shader stats for \"{label}\": {error:?}");
            return;
        }
    };
    for (index, executable) in executables.iter().enumerate() {
        let executable_info = vk::PipelineExecutableInfoKHR::default()
            .pipeline(pipeline)
            .executable_index(u32::try_from(index).unwrap_or(u32::MAX));
        // SAFETY: as above, with an index the driver just listed.
        let statistics = match unsafe { ext.get_pipeline_executable_statistics(&executable_info) } {
            Ok(statistics) => statistics,
            Err(error) => {
                crcbl_core::log::warn!(
                    "crcbl-vk: shader stats for \"{label}\" executable {index}: {error:?}"
                );
                continue;
            }
        };
        let name = executable.name_as_c_str().map_or_else(
            |_| String::from("?"),
            |name| name.to_string_lossy().into_owned(),
        );
        let line: Vec<String> = statistics
            .iter()
            .map(|statistic| {
                let stat = statistic.name_as_c_str().map_or_else(
                    |_| String::from("?"),
                    |name| name.to_string_lossy().into_owned(),
                );
                // SAFETY: the union member read is the one `format` names.
                let value = unsafe { value_text(statistic.format, statistic.value) };
                format!("{stat}={value}")
            })
            .collect();
        crcbl_core::log::info!(
            "crcbl-vk: shader stats \"{label}\" {name} (subgroup {}): {}",
            executable.subgroup_size,
            line.join(", ")
        );
    }
}

/// One statistic's value as text.
///
/// # Safety
///
/// `format` must be the member of `value` the driver wrote, as
/// `VkPipelineExecutableStatisticKHR` pairs them.
unsafe fn value_text(
    format: vk::PipelineExecutableStatisticFormatKHR,
    value: vk::PipelineExecutableStatisticValueKHR,
) -> String {
    // SAFETY: each arm reads the member its format names, per this function's
    // contract.
    unsafe {
        match format {
            vk::PipelineExecutableStatisticFormatKHR::BOOL32 => {
                (value.b32 != vk::FALSE).to_string()
            }
            vk::PipelineExecutableStatisticFormatKHR::INT64 => value.i64.to_string(),
            vk::PipelineExecutableStatisticFormatKHR::UINT64 => value.u64.to_string(),
            vk::PipelineExecutableStatisticFormatKHR::FLOAT64 => value.f64.to_string(),
            other => format!("<format {}>", other.as_raw()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_one_turns_the_statistics_on() {
        assert!(wanted_by(Some("1")));
        assert!(wanted_by(Some(" 1\n")));
        for value in [
            None,
            Some(""),
            Some("0"),
            Some("true"),
            Some("yes"),
            Some("11"),
        ] {
            assert!(!wanted_by(value), "{value:?}");
        }
    }

    #[test]
    fn each_value_reads_the_member_its_format_names() {
        let format = vk::PipelineExecutableStatisticFormatKHR::UINT64;
        let value = vk::PipelineExecutableStatisticValueKHR { u64: 1 << 40 };
        // SAFETY: the `u64` member was the one written, as `format` says.
        assert_eq!(unsafe { value_text(format, value) }, "1099511627776");

        let value = vk::PipelineExecutableStatisticValueKHR { i64: -3 };
        // SAFETY: as above, for `i64`.
        assert_eq!(
            unsafe { value_text(vk::PipelineExecutableStatisticFormatKHR::INT64, value) },
            "-3"
        );

        let value = vk::PipelineExecutableStatisticValueKHR { b32: vk::TRUE };
        // SAFETY: as above, for `b32`.
        assert_eq!(
            unsafe { value_text(vk::PipelineExecutableStatisticFormatKHR::BOOL32, value) },
            "true"
        );

        let value = vk::PipelineExecutableStatisticValueKHR { f64: 0.5 };
        // SAFETY: as above, for `f64`.
        assert_eq!(
            unsafe { value_text(vk::PipelineExecutableStatisticFormatKHR::FLOAT64, value) },
            "0.5"
        );
    }
}
