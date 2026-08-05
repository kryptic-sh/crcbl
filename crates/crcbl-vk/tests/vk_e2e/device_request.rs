use crate::harness::instance;
use crcbl_hal::{BufferDesc, BufferUsage, DeviceDesc, Features, Instance, MemoryLocation};

/// The two `request_device` argument errors, neither of which any vk test
/// asserted. Both are decided before a `VkDevice` exists, so they need nothing
/// from the driver beyond an open instance.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn request_device_names_an_unknown_adapter_and_an_absent_feature() {
    let instance = instance();
    let adapters = instance.adapters();
    assert!(
        !adapters.is_empty(),
        "open() rejects an adapter-less loader"
    );

    let past_the_end = u32::try_from(adapters.len()).expect("a handful of adapters");
    let error = instance
        .create_device(&DeviceDesc::for_adapter(crcbl_hal::AdapterId(past_the_end)))
        .expect_err("there is no adapter one past the last");
    assert!(
        matches!(error, crcbl_hal::HalError::NoSuchAdapter(index) if index == past_the_end),
        "the error names the index that was asked for: {error}"
    );

    // `adapter::features_of` never sets `SHADER_DEBUG_PRINTF` — it needs the
    // layer's printf feature wired to a message handler and lands with the
    // debug UI — so it is the one capability no adapter here can satisfy,
    // whatever driver this is.
    let adapter = &adapters[0];
    assert!(
        !adapter.caps.supports(Features::SHADER_DEBUG_PRINTF),
        "this backend reports SHADER_DEBUG_PRINTF absent until it works"
    );
    let error = instance
        .create_device(&DeviceDesc {
            label: Some("impossible"),
            adapter: adapter.id,
            required_features: Features::SHADER_DEBUG_PRINTF,
            optional_features: Features::empty(),
            compatible_surface: None,
        })
        .expect_err("a required feature the adapter lacks is refused");
    let crcbl_hal::HalError::UnsupportedFeatures { missing } = error else {
        panic!("a feature gap is UnsupportedFeatures, not {error:?}");
    };
    assert_eq!(
        missing,
        Features::SHADER_DEBUG_PRINTF,
        "the error carries the exact gap, so the log says which capability to \
         go look up"
    );

    instance.validation_report().assert_clean();
}

/// The polled device-creation path, which every other test in this suite skips
/// by going through the blocking `create_device` wrapper.
///
/// `vkCreateDevice` is synchronous, so this backend's first poll is always
/// `Ready` — and the contract's other half is that there is no *second* device
/// behind it. Until now that was asserted only against the null backend.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_device_request_hands_its_device_over_exactly_once() {
    let instance = instance();
    let adapter = instance.adapters().remove(0);
    let mut pending = instance
        .request_device(&DeviceDesc {
            label: Some("polled"),
            adapter: adapter.id,
            required_features: Features::empty(),
            optional_features: Features::empty(),
            compatible_surface: None,
        })
        .expect("the request is accepted");
    assert_eq!(pending.backend(), crcbl_hal::BackendKind::Vulkan);

    let device = match pending.poll().expect("the first poll succeeds") {
        crcbl_hal::DeviceRequestState::Ready(device) => device,
        crcbl_hal::DeviceRequestState::Pending => {
            panic!("vkCreateDevice already ran, so this backend never defers")
        }
    };

    let error = pending
        .poll()
        .expect_err("polling a finished request must not produce a second device");
    assert!(
        matches!(error, crcbl_hal::HalError::InvalidDescriptor(_)),
        "a poll after Ready is a caller bug with a message, not a device: {error}"
    );

    // And the device the one successful poll produced is a working one.
    let buffer = device
        .create_buffer(&BufferDesc {
            label: Some("from a polled request"),
            size: 64,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostUpload,
        })
        .expect("the polled device works");
    device.write_buffer(buffer, 0, &[5; 64]).expect("write");
    device.destroy_buffer(buffer);
    device.wait_idle().expect("idle");
    drop(device);
    drop(pending);

    instance.validation_report().assert_clean();
}

/// The tier determination, against whatever this machine actually is. The
/// assertion is not "Tier A" — lavapipe may not be — but that the *report is
/// consistent*, which is the property a renderer branches on.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn the_reported_tier_agrees_with_the_reported_features() {
    let instance = instance();
    let adapters = instance.adapters();
    // Without this the whole test is a loop over an empty list — every
    // assertion below is skipped and the test passes having checked nothing.
    // `open()` already refuses an adapter-less loader, so this is also the
    // check that it did.
    assert!(
        !adapters.is_empty(),
        "open() rejects an adapter-less loader"
    );
    for adapter in adapters {
        let caps = adapter.caps;
        assert_eq!(
            caps.tier().is_a(),
            caps.supports(Features::TIER_A),
            "{}: tier and features must agree",
            adapter.name
        );
        if caps.tier().is_a() {
            assert!(caps.limits.max_bindless_descriptors > 0, "{}", adapter.name);
            assert!(caps.limits.max_draw_indirect_count > 1, "{}", adapter.name);
        } else {
            eprintln!(
                "vk e2e: {} is Tier B, missing {:?}",
                adapter.name,
                caps.missing(Features::TIER_A)
            );
        }
        if caps.features.contains(Features::TIMESTAMP_QUERY) {
            assert!(caps.limits.timestamp_period_ns > 0.0, "{}", adapter.name);
        } else {
            assert_eq!(caps.limits.timestamp_period_ns, 0.0, "{}", adapter.name);
        }
    }
}
