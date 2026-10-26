//! The GPU a program ran on, saved and given back so its next start opens
//! exactly that one — and on Vulkan, loads only that one's driver.
//!
//! # What the program does
//!
//! It stores one opaque value and hands it back. After an open,
//! [`Presenter::gpu_choice`](crate::present::Presenter::gpu_choice) is the
//! [`GpuChoice`] for the adapter it opened; the program saves its text form
//! ([`GpuChoice`] is `Display` and `FromStr`) wherever it keeps its state, and
//! passes it as [`PresenterDesc::gpu_choice`](crate::present::PresenterDesc::gpu_choice)
//! next time. [`Presenter::gpu_choice_outcome`](crate::present::Presenter::gpu_choice_outcome)
//! says what became of it: [`ChoiceOutcome::Used`],
//! [`ChoiceOutcome::Stale`] — it no longer holds, the open went on without
//! it, and the program should save the new choice in its place — or
//! [`ChoiceOutcome::Ignored`]. Everything else, matching it to an adapter,
//! checking it still holds and falling back when it does not, is this crate's.
//!
//! # What it saves, per backend
//!
//! - **Vulkan**: with the driver's ICD manifest
//!   (`GpuChoice::locate_vulkan_driver`, native only), the instance is opened
//!   on that driver alone (`crcbl_vk::VkInstance::open_driver`), so the loader
//!   loads no other: on the office laptop (Intel RPL-U and NVIDIA MX550, Arch) an
//!   instance on Intel's driver opened in 8–9 ms with no NVIDIA library
//!   loaded, against 60–77 ms and NVIDIA's device files held open for an
//!   instance on every driver. Without a manifest it still names the adapter.
//! - **D3D12**: the adapter by its LUID, with vendor, device and name to
//!   check it, since a LUID lasts only until the machine restarts. Not built
//!   yet; until it is, a D3D12 choice is [`Ignored::Unsupported`].
//! - **Metal**: [`Ignored::Unsupported`]. Metal has one driver per machine
//!   and nothing to skip.
//!
//! # What overrides it
//!
//! A choice is [`Ignored::Overridden`] whenever someone has already chosen:
//! [`PresenterDesc::adapter_pin`](crate::present::PresenterDesc::adapter_pin)
//! (`CRCBL_ADAPTER`) or
//! [`PresenterDesc::adapter_pick`](crate::present::PresenterDesc::adapter_pick),
//! and on Vulkan the loader's own driver variables
//! ([`VULKAN_DRIVER_ENV_VARS`]), which a person sets on purpose. A choice for
//! one backend is [`Ignored::OtherBackend`] when another is opened.

use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crcbl_hal::AdapterInfo;

use crate::backend::GpuBackend;

/// The loader variables that choose Vulkan drivers, any of which makes a
/// saved choice [`Ignored::Overridden`].
pub const VULKAN_DRIVER_ENV_VARS: [&str; 5] = [
    "VK_DRIVER_FILES",
    "VK_ICD_FILENAMES",
    "VK_ADD_DRIVER_FILES",
    "VK_LOADER_DRIVERS_SELECT",
    "VK_LOADER_DRIVERS_DISABLE",
];

/// The first line of a [`GpuChoice`]'s text form, with its format's version.
const HEADER: &str = "crcbl-gpu-choice 1";

/// A GPU a program ran on: an opaque value it saves and gives back. See the
/// module docs.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GpuChoice {
    backend: GpuBackend,
    vendor_id: u32,
    device_id: u32,
    name: String,
    luid: Option<u64>,
    vulkan_driver: Option<PathBuf>,
}

impl GpuChoice {
    /// The choice of `adapter` on `backend`, with no Vulkan driver located.
    #[must_use]
    pub fn of(backend: GpuBackend, adapter: &AdapterInfo) -> Self {
        Self {
            backend,
            vendor_id: adapter.vendor_id,
            device_id: adapter.device_id,
            name: adapter.name.clone(),
            luid: adapter.luid,
            vulkan_driver: None,
        }
    }

    /// The backend it was made on.
    #[must_use]
    pub const fn backend(&self) -> GpuBackend {
        self.backend
    }

    /// Whether `adapter` is the one it names: the same vendor, device and
    /// name.
    #[must_use]
    pub fn names(&self, adapter: &AdapterInfo) -> bool {
        adapter.vendor_id == self.vendor_id
            && adapter.device_id == self.device_id
            && adapter.name == self.name
    }

    /// The Vulkan driver's ICD manifest, once located.
    #[must_use]
    pub fn vulkan_driver(&self) -> Option<&Path> {
        self.vulkan_driver.as_deref()
    }

    /// The same choice through the Vulkan driver whose manifest is
    /// `manifest`.
    #[must_use]
    pub fn with_vulkan_driver(mut self, manifest: PathBuf) -> Self {
        self.vulkan_driver = Some(manifest);
        self
    }

    /// Finds the Vulkan driver that serves this adapter, by opening an
    /// instance on each installed driver alone ([`vulkan_drivers`]), and
    /// records its manifest. Returns whether one was found. Does nothing for a
    /// choice on another backend.
    ///
    /// **Slow, and for off a program's start-up path**: it loads every
    /// installed driver, which is the cost the located driver then saves on
    /// every later start. Run it once, after an open with no saved choice, on
    /// a thread of its own.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn locate_vulkan_driver(&mut self) -> bool {
        if self.backend != GpuBackend::Vulkan {
            return false;
        }
        let found = vulkan_drivers()
            .into_iter()
            .find_map(|(manifest, adapters)| {
                adapters
                    .ok()
                    .filter(|adapters| adapters.iter().any(|adapter| self.names(adapter)))
                    .map(|_| manifest)
            });
        let located = found.is_some();
        self.vulkan_driver = found;
        located
    }
}

/// Every installed Vulkan driver's manifest and the adapters an instance on
/// that driver alone shows, or why it would not open; see
/// [`GpuChoice::locate_vulkan_driver`] for its cost. Empty where the loader
/// finds drivers without manifests (Windows).
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn vulkan_drivers() -> Vec<(PathBuf, Result<Vec<AdapterInfo>, String>)> {
    use crcbl_hal::Instance as _;
    crcbl_vk::drivers::manifests()
        .into_iter()
        .map(|manifest| {
            let adapters = crcbl_vk::drivers::Driver::read(&manifest)
                .map_err(|error| error.to_string())
                .and_then(|driver| {
                    crcbl_vk::VkInstance::open_driver(&driver).map_err(|error| error.to_string())
                })
                .map(|instance| instance.adapters());
            (manifest, adapters)
        })
        .collect()
}

impl fmt::Display for GpuChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{HEADER}")?;
        writeln!(f, "backend {}", self.backend.as_str())?;
        writeln!(f, "vendor {:#06x}", self.vendor_id)?;
        writeln!(f, "device {:#06x}", self.device_id)?;
        writeln!(f, "name {}", self.name)?;
        if let Some(luid) = self.luid {
            writeln!(f, "luid {luid:#x}")?;
        }
        if let Some(driver) = &self.vulkan_driver {
            writeln!(f, "vulkan-driver {}", driver.display())?;
        }
        Ok(())
    }
}

/// Why a saved [`GpuChoice`]'s text could not be read: not one, a version
/// this build does not read, or a field missing or malformed. A program
/// treats it as no saved choice.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("not a saved GPU choice: {0}")]
pub struct BadGpuChoice(String);

impl FromStr for GpuChoice {
    type Err = BadGpuChoice;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let bad = |why: &str| BadGpuChoice(why.to_owned());
        let mut lines = text.lines();
        if lines.next().map(str::trim_end) != Some(HEADER) {
            return Err(bad("the first line is not the format's header"));
        }
        let (mut backend, mut vendor, mut device, mut name) = (None, None, None, None);
        let (mut luid, mut vulkan_driver) = (None, None);
        let hex = |value: &str| u64::from_str_radix(value.trim_start_matches("0x"), 16).ok();
        for line in lines.filter(|line| !line.trim().is_empty()) {
            let (key, value) = line.split_once(' ').ok_or_else(|| bad(line))?;
            match key {
                "backend" => backend = GpuBackend::from_name(value),
                "vendor" => vendor = hex(value).and_then(|id| u32::try_from(id).ok()),
                "device" => device = hex(value).and_then(|id| u32::try_from(id).ok()),
                "name" => name = Some(value.to_owned()),
                "luid" => luid = Some(hex(value).ok_or_else(|| bad("a malformed luid"))?),
                "vulkan-driver" => vulkan_driver = Some(PathBuf::from(value)),
                // A later version's field: read past, so an older build can
                // still use what it knows.
                _ => {}
            }
        }
        Ok(Self {
            backend: backend.ok_or_else(|| bad("no backend this build has"))?,
            vendor_id: vendor.ok_or_else(|| bad("no vendor"))?,
            device_id: device.ok_or_else(|| bad("no device"))?,
            name: name.ok_or_else(|| bad("no name"))?,
            luid,
            vulkan_driver,
        })
    }
}

/// What an open did with the [`GpuChoice`] it was given.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ChoiceOutcome {
    /// None was given.
    #[default]
    NotGiven,
    /// It held: the open is on the adapter it names, through its Vulkan
    /// driver alone where it had one.
    Used,
    /// It no longer holds — the driver would not load, or no longer shows
    /// that adapter, or the adapter cannot present to the window — and the
    /// open went on without it. The program should save the presenter's new
    /// [`gpu_choice`](crate::present::Presenter::gpu_choice) in its place.
    Stale(String),
    /// It was not tried; see [`Ignored`].
    Ignored(Ignored),
}

/// Why a [`GpuChoice`] was not tried.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ignored {
    /// Someone already chose, through what this names; see the module docs.
    Overridden(&'static str),
    /// The open is on another backend than the one it was made on.
    OtherBackend(GpuBackend),
    /// Its backend does not open by a saved choice; see the module docs.
    Unsupported(GpuBackend),
}

/// What overrides a choice for an open with `pin` and `pick` on `backend`,
/// if anything does; see the module docs.
pub(crate) fn overridden(pin: bool, pick: bool, backend: GpuBackend) -> Option<&'static str> {
    if pin {
        return Some("the adapter pin (CRCBL_ADAPTER)");
    }
    if pick {
        return Some("the program's adapter pick");
    }
    if backend == GpuBackend::Vulkan {
        return VULKAN_DRIVER_ENV_VARS
            .into_iter()
            .find(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_hal::{AdapterId, BackendKind, DeviceType};

    fn intel() -> AdapterInfo {
        AdapterInfo {
            id: AdapterId(0),
            name: "Intel(R) Graphics (RPL-U)".to_owned(),
            vendor_id: 0x8086,
            device_id: 0xa7a1,
            device_type: DeviceType::Integrated,
            driver: "Intel open-source Mesa driver".to_owned(),
            backend: BackendKind::Vulkan,
            luid: None,
        }
    }

    /// **A choice survives its text form**, every field of it, a name with
    /// spaces and parentheses included, and names the adapter it was made
    /// from and no other.
    #[test]
    fn a_choice_round_trips_through_its_text_and_names_its_adapter() {
        let choice = GpuChoice::of(GpuBackend::Vulkan, &intel())
            .with_vulkan_driver(PathBuf::from("/usr/share/vulkan/icd.d/intel_icd.json"));
        let text = choice.to_string();
        assert!(text.starts_with(HEADER), "{text}");
        let read: GpuChoice = text.parse().expect("its own text");
        assert_eq!(read, choice);
        assert!(read.names(&intel()));
        let other = AdapterInfo {
            device_id: 0x1234,
            ..intel()
        };
        assert!(!read.names(&other));

        let luid = GpuChoice {
            luid: Some(0x0001_2345_6789),
            backend: GpuBackend::Dx12,
            ..choice
        };
        assert_eq!(luid.to_string().parse::<GpuChoice>(), Ok(luid));
    }

    /// **Text that is not a choice is refused, never half-read**: another
    /// header, a later format version, a missing field. A field a later
    /// version added is read past.
    #[test]
    fn text_that_is_not_a_choice_is_refused() {
        let good = GpuChoice::of(GpuBackend::Vulkan, &intel()).to_string();
        assert!("".parse::<GpuChoice>().is_err());
        assert!(
            good.replace(HEADER, "crcbl-gpu-choice 2")
                .parse::<GpuChoice>()
                .is_err()
        );
        assert!(
            good.replace("vendor 0x8086\n", "")
                .parse::<GpuChoice>()
                .is_err()
        );
        assert!(
            good.replace("backend vk", "backend nope")
                .parse::<GpuChoice>()
                .is_err()
        );
        let later = format!("{good}colour-space hdr\n");
        assert_eq!(later.parse::<GpuChoice>().ok(), good.parse().ok());
    }

    /// **Someone else's choice wins**: a pin, a pick, then on Vulkan the
    /// loader's driver variables, which on another backend mean nothing.
    #[test]
    fn a_pin_a_pick_or_a_driver_variable_overrides_a_choice() {
        assert!(overridden(true, false, GpuBackend::Vulkan).is_some());
        assert!(overridden(false, true, GpuBackend::Dx12).is_some());
        // The process's own environment, read as the open reads it: only a
        // run with none of the loader's variables set can say "nothing".
        let set = VULKAN_DRIVER_ENV_VARS
            .into_iter()
            .find(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()));
        assert_eq!(overridden(false, false, GpuBackend::Vulkan), set);
        assert_eq!(overridden(false, false, GpuBackend::Dx12), None);
    }
}
