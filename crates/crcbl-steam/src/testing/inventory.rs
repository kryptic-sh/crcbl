//! The fake `ISteamInventory`: scripted results, definitions and prices, and
//! every call recorded as one line — its name, then its arguments,
//! `|`-separated — in order, for the tests to read back.

use std::ffi::{CStr, c_char, c_void};

use super::{accessor, script};
use crate::ffi::{
    ISteamInventory, SteamApiCall, SteamInventoryResult, SteamItemDef, SteamItemInstanceId,
    manifest::InventoryFns, structs::SteamItemDetails,
};

/// What the fake inventory answers, and what it has seen.
#[derive(Debug, Default)]
pub(crate) struct FakeInventory {
    /// Every call, as one line each.
    pub(crate) log: Vec<String>,
    /// The handle every result-producing call writes; `-1` is invalid.
    pub(crate) handle: SteamInventoryResult,
    /// Every result-producing call answers `false`, whatever it wrote.
    pub(crate) refuse: bool,
    /// What `GetResultStatus` answers.
    pub(crate) status: i32,
    /// What `GetResultItems` copies out; the count it reports for a null
    /// array is `claimed`, or their count when unset.
    pub(crate) items: Vec<SteamItemDetails>,
    pub(crate) claimed: Option<u32>,
    /// The definitions `GetItemDefinitionIDs` reports; `None` answers
    /// `false`, as before the definitions load.
    pub(crate) definitions: Option<Vec<SteamItemDef>>,
    /// What `GetItemDefinitionProperty` copies out; `None` answers `false`.
    pub(crate) property: Option<Vec<u8>>,
    /// Every property buffer size offered.
    pub(crate) property_offered: Vec<u32>,
    /// What the call-returning functions answer; `0` is invalid.
    pub(crate) call: SteamApiCall,
    /// The prices `GetItemsWithPrices` copies out: definition, current, base.
    pub(crate) prices: Vec<(SteamItemDef, u64, u64)>,
}

fn log(line: String) {
    script(|s| s.inventory.log.push(line));
}

/// Writes the scripted handle, and answers whether the call started.
fn produce(out: *mut SteamInventoryResult, line: String) -> bool {
    log(line);
    let (handle, refuse) = script(|s| (s.inventory.handle, s.inventory.refuse));
    // SAFETY: every caller passes a writable handle.
    unsafe { out.write(handle) };
    !refuse
}

/// `len` elements at `ptr`, joined with commas.
fn list<T: ToString + Copy>(ptr: *const T, len: u32) -> String {
    let len = usize::try_from(len).unwrap();
    if len == 0 {
        return String::new();
    }
    // SAFETY: every caller passes `len` readable elements.
    let items = unsafe { core::slice::from_raw_parts(ptr, len) };
    items
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

pub(super) const INVENTORY: InventoryFns = InventoryFns {
    accessor: inventory_accessor,
    get_result_status: fake_get_result_status,
    get_result_items: fake_get_result_items,
    get_result_timestamp: fake_get_result_timestamp,
    check_result_steam_id: fake_check_result_steam_id,
    destroy_result: fake_destroy_result,
    get_all_items: fake_get_all_items,
    get_items_by_id: fake_get_items_by_id,
    grant_promo_items: fake_grant_promo_items,
    add_promo_item: fake_add_promo_item,
    consume_item: fake_consume_item,
    exchange_items: fake_exchange_items,
    load_item_definitions: fake_load_item_definitions,
    get_item_definition_ids: fake_get_item_definition_ids,
    get_item_definition_property: fake_get_item_definition_property,
    start_purchase: fake_start_purchase,
    request_prices: fake_request_prices,
    get_num_items_with_prices: fake_get_num_items_with_prices,
    get_items_with_prices: fake_get_items_with_prices,
    get_item_price: fake_get_item_price,
};

unsafe extern "C" fn inventory_accessor() -> *mut c_void {
    accessor(crate::ffi::versions::INVENTORY.accessor)
}

unsafe extern "C" fn fake_get_result_status(
    _: *mut ISteamInventory,
    handle: SteamInventoryResult,
) -> i32 {
    log(format!("GetResultStatus|{handle}"));
    script(|s| s.inventory.status)
}

unsafe extern "C" fn fake_get_result_items(
    _: *mut ISteamInventory,
    handle: SteamInventoryResult,
    out: *mut SteamItemDetails,
    size: *mut u32,
) -> bool {
    let (items, claimed) = script(|s| (s.inventory.items.clone(), s.inventory.claimed));
    let all = u32::try_from(items.len()).unwrap();
    // SAFETY: the caller passes a writable count, and with a non-null array
    // that many writable `SteamItemDetails_t`s.
    unsafe {
        if out.is_null() {
            log(format!("GetResultItems|{handle}|count"));
            size.write(claimed.unwrap_or(all));
        } else {
            let capacity = size.read();
            log(format!("GetResultItems|{handle}|{capacity}"));
            let n = items.len().min(usize::try_from(capacity).unwrap());
            core::ptr::copy_nonoverlapping(items.as_ptr(), out, n);
            // Steam's count of every item, which may exceed what fit.
            size.write(all);
        }
    }
    true
}

unsafe extern "C" fn fake_get_result_timestamp(
    _: *mut ISteamInventory,
    handle: SteamInventoryResult,
) -> u32 {
    log(format!("GetResultTimestamp|{handle}"));
    1_700_000_000
}

unsafe extern "C" fn fake_check_result_steam_id(
    _: *mut ISteamInventory,
    handle: SteamInventoryResult,
    user: u64,
) -> bool {
    log(format!("CheckResultSteamID|{handle}|{user}"));
    user == super::STEAM_ID
}

unsafe extern "C" fn fake_destroy_result(_: *mut ISteamInventory, handle: SteamInventoryResult) {
    log(format!("DestroyResult|{handle}"));
}

unsafe extern "C" fn fake_get_all_items(
    _: *mut ISteamInventory,
    out: *mut SteamInventoryResult,
) -> bool {
    produce(out, "GetAllItems".to_owned())
}

unsafe extern "C" fn fake_get_items_by_id(
    _: *mut ISteamInventory,
    out: *mut SteamInventoryResult,
    ids: *const SteamItemInstanceId,
    count: u32,
) -> bool {
    produce(out, format!("GetItemsByID|{}", list(ids, count)))
}

unsafe extern "C" fn fake_grant_promo_items(
    _: *mut ISteamInventory,
    out: *mut SteamInventoryResult,
) -> bool {
    produce(out, "GrantPromoItems".to_owned())
}

unsafe extern "C" fn fake_add_promo_item(
    _: *mut ISteamInventory,
    out: *mut SteamInventoryResult,
    definition: SteamItemDef,
) -> bool {
    produce(out, format!("AddPromoItem|{definition}"))
}

unsafe extern "C" fn fake_consume_item(
    _: *mut ISteamInventory,
    out: *mut SteamInventoryResult,
    item: SteamItemInstanceId,
    quantity: u32,
) -> bool {
    produce(out, format!("ConsumeItem|{item}|{quantity}"))
}

unsafe extern "C" fn fake_exchange_items(
    _: *mut ISteamInventory,
    out: *mut SteamInventoryResult,
    generate: *const SteamItemDef,
    generate_counts: *const u32,
    generate_len: u32,
    destroy: *const SteamItemInstanceId,
    destroy_counts: *const u32,
    destroy_len: u32,
) -> bool {
    produce(
        out,
        format!(
            "ExchangeItems|{}|{}|{}|{}",
            list(generate, generate_len),
            list(generate_counts, generate_len),
            list(destroy, destroy_len),
            list(destroy_counts, destroy_len),
        ),
    )
}

unsafe extern "C" fn fake_load_item_definitions(_: *mut ISteamInventory) -> bool {
    log("LoadItemDefinitions".to_owned());
    script(|s| !s.refuse)
}

unsafe extern "C" fn fake_get_item_definition_ids(
    _: *mut ISteamInventory,
    out: *mut SteamItemDef,
    size: *mut u32,
) -> bool {
    log(format!(
        "GetItemDefinitionIDs|{}",
        if out.is_null() { "count" } else { "ids" }
    ));
    let Some(definitions) = script(|s| s.inventory.definitions.clone()) else {
        return false;
    };
    let all = u32::try_from(definitions.len()).unwrap();
    // SAFETY: the caller passes a writable count, and with a non-null array
    // that many writable definitions.
    unsafe {
        if !out.is_null() {
            let n = definitions.len().min(usize::try_from(size.read()).unwrap());
            core::ptr::copy_nonoverlapping(definitions.as_ptr(), out, n);
        }
        size.write(all);
    }
    true
}

unsafe extern "C" fn fake_get_item_definition_property(
    _: *mut ISteamInventory,
    definition: SteamItemDef,
    name: *const c_char,
    out: *mut c_char,
    size: *mut u32,
) -> bool {
    let name = if name.is_null() {
        "(names)".to_owned()
    } else {
        // SAFETY: a non-null name is NUL-terminated.
        unsafe { CStr::from_ptr(name) }
            .to_string_lossy()
            .into_owned()
    };
    // SAFETY: the caller passes a writable size holding the buffer's.
    let capacity = unsafe { size.read() };
    log(format!("GetItemDefinitionProperty|{definition}|{name}"));
    let value = script(|s| {
        s.inventory.property_offered.push(capacity);
        s.inventory.property.clone()
    });
    let Some(value) = value else {
        return false;
    };
    let room = usize::try_from(capacity).unwrap().saturating_sub(1);
    let n = value.len().min(room);
    // SAFETY: the caller passes `capacity` writable bytes; `n + 1` is at most
    // that. Steam's copy stops a byte short to leave the NUL, as here.
    unsafe {
        if capacity > 0 {
            core::ptr::copy_nonoverlapping(value.as_ptr(), out.cast::<u8>(), n);
            out.add(n).cast::<u8>().write(0);
        }
        size.write(u32::try_from(n + 1).unwrap());
    }
    true
}

unsafe extern "C" fn fake_start_purchase(
    _: *mut ISteamInventory,
    definitions: *const SteamItemDef,
    counts: *const u32,
    len: u32,
) -> SteamApiCall {
    log(format!(
        "StartPurchase|{}|{}",
        list(definitions, len),
        list(counts, len)
    ));
    script(|s| s.inventory.call)
}

unsafe extern "C" fn fake_request_prices(_: *mut ISteamInventory) -> SteamApiCall {
    log("RequestPrices".to_owned());
    script(|s| s.inventory.call)
}

unsafe extern "C" fn fake_get_num_items_with_prices(_: *mut ISteamInventory) -> u32 {
    script(|s| u32::try_from(s.inventory.prices.len()).unwrap())
}

unsafe extern "C" fn fake_get_items_with_prices(
    _: *mut ISteamInventory,
    definitions: *mut SteamItemDef,
    current: *mut u64,
    base: *mut u64,
    len: u32,
) -> bool {
    let prices = script(|s| s.inventory.prices.clone());
    if usize::try_from(len).unwrap() != prices.len() {
        return false;
    }
    for (i, &(definition, now, was)) in prices.iter().enumerate() {
        // SAFETY: the caller passes three arrays of `len` writable elements.
        unsafe {
            definitions.add(i).write(definition);
            current.add(i).write(now);
            base.add(i).write(was);
        }
    }
    true
}

unsafe extern "C" fn fake_get_item_price(
    _: *mut ISteamInventory,
    definition: SteamItemDef,
    current: *mut u64,
    base: *mut u64,
) -> bool {
    let price = script(|s| {
        s.inventory
            .prices
            .iter()
            .find(|price| price.0 == definition)
            .copied()
    });
    let Some((_, now, was)) = price else {
        return false;
    };
    // SAFETY: the caller passes two writable prices.
    unsafe {
        current.write(now);
        base.write(was);
    }
    true
}
