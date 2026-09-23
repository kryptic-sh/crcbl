//! `ISteamInventory` (`docs/plan/42-steam.md` slice 15, its inventory half):
//! the items a player holds, the definitions they are made from, and buying
//! more.
//!
//! Every inventory operation — [`Inventory::all_items`],
//! [`Inventory::consume`], [`Inventory::exchange`], the promo grants —
//! answers an [`InventoryResult`] at once, which is **pending** until
//! [`SteamEvent::InventoryResultReady`](crate::SteamEvent::InventoryResultReady)
//! names it; then [`InventoryResult::items`] reads what it holds. The result
//! owns Steam's handle and destroys it (`DestroyResult`) exactly once, when
//! dropped — ready or not. A handle Steam made for an operation it then
//! refused is destroyed the same way, before the refusal is returned.
//!
//! Item definitions are loaded by Steam on its own at start-up
//! ([`Inventory::load_item_definitions`] asks again) and read with
//! [`Inventory::item_definitions`] and [`Inventory::definition_property`].
//! Purchases and prices are asynchronous calls
//! ([`Inventory::start_purchase`], [`Inventory::request_prices`]).
//!
//! Not bound, each on demand: serialising results for another player to
//! check, dev-only item generation, quantity transfers, trades, timed drops,
//! eligible-promo queries, per-item dynamic properties, and inspecting an
//! item by its token. Under app 480 whether SpaceWar's example definitions
//! exist is believed, not verified (`docs/plan/42-steam.md`, R3).

use core::ffi::c_char;
use std::{marker::PhantomData, sync::Arc};

use crate::{
    EResult, Steam, SteamCall, SteamError, SteamId,
    apps::grow,
    call::{CallRow, private::Answer},
    callbacks::{Base, fixed_string, read},
    client::Client,
    error::c_string,
    ffi::{SteamInventoryResult, structs},
};

/// `k_SteamInventoryResultInvalid`.
const INVALID_RESULT: SteamInventoryResult = -1;

/// One item a player holds (`SteamItemInstanceID_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemInstanceId(pub u64);

/// An item definition (`SteamItemDef_t`), as the app's item schema numbers
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemDef(pub i32);

/// An inventory result's handle (`SteamInventoryResult_t`), as
/// [`SteamEvent::InventoryResultReady`](crate::SteamEvent::InventoryResultReady)
/// names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InventoryResultId(pub i32);

/// What an item is flagged as (`ESteamItemFlags`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ItemFlags(pub u16);

impl ItemFlags {
    /// Account-locked: it cannot be traded or given away
    /// (`k_ESteamItemNoTrade`).
    pub const NO_TRADE: Self = Self(1);
    /// Destroyed, traded away, expired or otherwise gone
    /// (`k_ESteamItemRemoved`).
    pub const REMOVED: Self = Self(1 << 8);
    /// Its quantity went down by one through a consume
    /// (`k_ESteamItemConsumed`).
    pub const CONSUMED: Self = Self(1 << 9);

    /// Whether every flag in `other` is set.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

/// One item in a result (`SteamItemDetails_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InventoryItem {
    /// The item.
    pub id: ItemInstanceId,
    /// What it is.
    pub definition: ItemDef,
    /// How many of it the stack holds.
    pub quantity: u16,
    /// What it is flagged as.
    pub flags: ItemFlags,
}

/// An item definition's price, in the currency
/// [`PricesReady::currency`] names, in its smallest unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemPrice {
    /// The definition.
    pub definition: ItemDef,
    /// What it costs now.
    pub current: u64,
    /// What it costs without a discount.
    pub base: u64,
}

/// The player's inventory, borrowed from a [`Steam`]; from
/// [`Steam::inventory`].
#[derive(Debug)]
pub struct Inventory<'a> {
    steam: &'a mut Steam,
}

impl Steam {
    /// The player's inventory items, the item definitions, and purchases.
    pub fn inventory(&mut self) -> Inventory<'_> {
        Inventory { steam: self }
    }
}

impl Inventory<'_> {
    /// Every item the player holds (`GetAllItems`).
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam will not start it.
    pub fn all_items(&self) -> Result<InventoryResult, SteamError> {
        self.operation("GetAllItems", |client, out| {
            // SAFETY: see `operation`.
            unsafe { (client.lib.fns.inventory.get_all_items)(client.inventory, out) }
        })
    }

    /// The items with these ids, if the player holds them (`GetItemsByID`).
    ///
    /// # Errors
    ///
    /// [`SteamError::TooMany`] past a `uint32` count, before the call;
    /// [`SteamError::Refused`] when Steam will not start it.
    pub fn items_by_id(&self, ids: &[ItemInstanceId]) -> Result<InventoryResult, SteamError> {
        let ids: Vec<u64> = ids.iter().map(|id| id.0).collect();
        let count = count(ids.len(), "item ids")?;
        self.operation("GetItemsByID", |client, out| {
            // SAFETY: see `operation`; `ids` is `count` readable ids.
            unsafe {
                (client.lib.fns.inventory.get_items_by_id)(
                    client.inventory,
                    out,
                    ids.as_ptr(),
                    count,
                )
            }
        })
    }

    /// Grants every promotional item the player is eligible for
    /// (`GrantPromoItems`).
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam will not start it.
    pub fn grant_promo_items(&self) -> Result<InventoryResult, SteamError> {
        self.operation("GrantPromoItems", |client, out| {
            // SAFETY: see `operation`.
            unsafe { (client.lib.fns.inventory.grant_promo_items)(client.inventory, out) }
        })
    }

    /// Grants one promotional item, if the player is eligible for it
    /// (`AddPromoItem`).
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam will not start it.
    pub fn add_promo_item(&self, definition: ItemDef) -> Result<InventoryResult, SteamError> {
        self.operation("AddPromoItem", |client, out| {
            // SAFETY: see `operation`.
            unsafe {
                (client.lib.fns.inventory.add_promo_item)(client.inventory, out, definition.0)
            }
        })
    }

    /// Uses up `quantity` of the stack `item` (`ConsumeItem`).
    ///
    /// # Errors
    ///
    /// [`SteamError::OutOfRange`] for a quantity of zero, before the call;
    /// [`SteamError::Refused`] when Steam will not start it.
    pub fn consume(
        &self,
        item: ItemInstanceId,
        quantity: u32,
    ) -> Result<InventoryResult, SteamError> {
        if quantity == 0 {
            return Err(SteamError::OutOfRange("quantity"));
        }
        self.operation("ConsumeItem", |client, out| {
            // SAFETY: see `operation`.
            unsafe {
                (client.lib.fns.inventory.consume_item)(client.inventory, out, item.0, quantity)
            }
        })
    }

    /// Exchanges items for others by a recipe the item schema defines
    /// (`ExchangeItems`): `generate` the definitions and quantities to make,
    /// `destroy` the stacks and quantities to spend.
    ///
    /// # Errors
    ///
    /// [`SteamError::TooMany`] past a `uint32` count, before the call;
    /// [`SteamError::Refused`] when Steam will not start it.
    pub fn exchange(
        &self,
        generate: &[(ItemDef, u32)],
        destroy: &[(ItemInstanceId, u32)],
    ) -> Result<InventoryResult, SteamError> {
        let (make, make_counts): (Vec<i32>, Vec<u32>) =
            generate.iter().map(|&(def, n)| (def.0, n)).unzip();
        let (spend, spend_counts): (Vec<u64>, Vec<u32>) =
            destroy.iter().map(|&(id, n)| (id.0, n)).unzip();
        let make_len = count(make.len(), "items to generate")?;
        let spend_len = count(spend.len(), "items to destroy")?;
        self.operation("ExchangeItems", |client, out| {
            // SAFETY: see `operation`; each array is its length's readable
            // elements.
            unsafe {
                (client.lib.fns.inventory.exchange_items)(
                    client.inventory,
                    out,
                    make.as_ptr(),
                    make_counts.as_ptr(),
                    make_len,
                    spend.as_ptr(),
                    spend_counts.as_ptr(),
                    spend_len,
                )
            }
        })
    }

    /// Asks Steam to load the item definitions again
    /// (`LoadItemDefinitions`); it loads them on its own at start-up, and
    /// [`SteamEvent::InventoryDefinitionsUpdated`](crate::SteamEvent::InventoryDefinitionsUpdated)
    /// follows a change.
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam refuses.
    pub fn load_item_definitions(&self) -> Result<(), SteamError> {
        let client = &self.steam.client;
        // SAFETY: `client.inventory` is live; `Inventory` borrows the `!Send`
        // `Steam`, so this is the pump thread.
        let loading = unsafe { (client.lib.fns.inventory.load_item_definitions)(client.inventory) };
        loading
            .then_some(())
            .ok_or(SteamError::Refused("LoadItemDefinitions"))
    }

    /// Every item definition the app's schema holds
    /// (`GetItemDefinitionIDs`, asked for the count, then the ids).
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] while the definitions are not loaded.
    pub fn item_definitions(&self) -> Result<Vec<ItemDef>, SteamError> {
        let client = &self.steam.client;
        let mut count = 0_u32;
        // SAFETY: as in `load_item_definitions`; a null array asks for the
        // count, written to the writable `count`.
        let known = unsafe {
            (client.lib.fns.inventory.get_item_definition_ids)(
                client.inventory,
                core::ptr::null_mut(),
                &raw mut count,
            )
        };
        if !known {
            return Err(SteamError::Refused("GetItemDefinitionIDs"));
        }
        let mut ids = vec![0_i32; usize::try_from(count).unwrap_or(0)];
        let mut written = u32::try_from(ids.len()).unwrap_or(0);
        // SAFETY: as above; `ids` is `written` writable ids, and `written` is
        // writable.
        let read = unsafe {
            (client.lib.fns.inventory.get_item_definition_ids)(
                client.inventory,
                ids.as_mut_ptr(),
                &raw mut written,
            )
        };
        if !read {
            return Err(SteamError::Refused("GetItemDefinitionIDs"));
        }
        // Never past the buffer, whatever Steam's count says.
        ids.truncate(usize::try_from(written).unwrap_or(0));
        Ok(ids.into_iter().map(ItemDef).collect())
    }

    /// One property of an item definition — `name`, `description`, `price`
    /// and whatever the schema adds — or, with `None`, the comma-separated
    /// names of every property it has (`GetItemDefinitionProperty`). Read
    /// through the growing buffer every string read here uses.
    ///
    /// # Errors
    ///
    /// [`SteamError::InteriorNul`] before the call; [`SteamError::Refused`]
    /// when Steam has no such definition or property;
    /// [`SteamError::Truncated`] past
    /// [`MAX_TEXT_BYTES`](crate::MAX_TEXT_BYTES).
    pub fn definition_property(
        &self,
        definition: ItemDef,
        property: Option<&str>,
    ) -> Result<String, SteamError> {
        let property = property
            .map(|name| c_string(name, "property", usize::MAX))
            .transpose()?;
        let name = property
            .as_ref()
            .map_or(core::ptr::null(), |name| name.as_ptr());
        let client = &self.steam.client;
        let ((), [value]) = grow("GetItemDefinitionProperty", |[value], capacity| {
            let mut size = u32::try_from(capacity).ok()?;
            // SAFETY: as in `load_item_definitions`; `name` is null or
            // NUL-terminated for the call, `value` is `size` writable bytes
            // and `size` is writable.
            let found = unsafe {
                (client.lib.fns.inventory.get_item_definition_property)(
                    client.inventory,
                    definition.0,
                    name,
                    value.as_mut_ptr().cast::<c_char>(),
                    &raw mut size,
                )
            };
            found.then_some(())
        })?;
        let (value, lossy) = fixed_string(&value);
        if lossy {
            let count = &self.steam.lossy_strings;
            count.set(count.get() + 1);
        }
        Ok(value)
    }

    /// Opens Steam's checkout for these definitions and quantities
    /// (`StartPurchase`); the answer carries the order.
    ///
    /// # Errors
    ///
    /// [`SteamError::TooMany`] past a `uint32` count, before the call;
    /// [`SteamError::Refused`] when Steam did not start the call.
    pub fn start_purchase(
        &mut self,
        items: &[(ItemDef, u32)],
    ) -> Result<SteamCall<PurchaseStarted>, SteamError> {
        let (defs, counts): (Vec<i32>, Vec<u32>) = items.iter().map(|&(def, n)| (def.0, n)).unzip();
        let len = count(defs.len(), "items to buy")?;
        let client = &self.steam.client;
        // SAFETY: as in `load_item_definitions`; both arrays are `len`
        // readable elements.
        let handle = unsafe {
            (client.lib.fns.inventory.start_purchase)(
                client.inventory,
                defs.as_ptr(),
                counts.as_ptr(),
                len,
            )
        };
        self.steam
            .calls
            .register(handle)
            .ok_or(SteamError::Refused("StartPurchase"))
    }

    /// Asks for the current prices in the player's currency
    /// (`RequestPrices`); once the call answers, [`prices`](Self::prices) and
    /// [`price`](Self::price) read them.
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam did not start the call.
    pub fn request_prices(&mut self) -> Result<SteamCall<PricesReady>, SteamError> {
        let client = &self.steam.client;
        // SAFETY: as in `load_item_definitions`.
        let handle = unsafe { (client.lib.fns.inventory.request_prices)(client.inventory) };
        self.steam
            .calls
            .register(handle)
            .ok_or(SteamError::Refused("RequestPrices"))
    }

    /// Every definition with a price (`GetNumItemsWithPrices`, then
    /// `GetItemsWithPrices`); empty until prices arrive.
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam will not hand them over.
    pub fn prices(&self) -> Result<Vec<ItemPrice>, SteamError> {
        let client = &self.steam.client;
        // SAFETY: as in `load_item_definitions`.
        let count =
            unsafe { (client.lib.fns.inventory.get_num_items_with_prices)(client.inventory) };
        let len = usize::try_from(count).unwrap_or(0);
        let mut defs = vec![0_i32; len];
        let mut current = vec![0_u64; len];
        let mut base = vec![0_u64; len];
        let capacity = u32::try_from(len).unwrap_or(0);
        // SAFETY: as in `load_item_definitions`; each array is `capacity`
        // writable elements.
        let read = unsafe {
            (client.lib.fns.inventory.get_items_with_prices)(
                client.inventory,
                defs.as_mut_ptr(),
                current.as_mut_ptr(),
                base.as_mut_ptr(),
                capacity,
            )
        };
        if !read {
            return Err(SteamError::Refused("GetItemsWithPrices"));
        }
        Ok(defs
            .into_iter()
            .zip(current)
            .zip(base)
            .map(|((definition, current), base)| ItemPrice {
                definition: ItemDef(definition),
                current,
                base,
            })
            .collect())
    }

    /// One definition's price, or `None` when it has none
    /// (`GetItemPrice`).
    #[must_use]
    pub fn price(&self, definition: ItemDef) -> Option<ItemPrice> {
        let client = &self.steam.client;
        let mut current = 0_u64;
        let mut base = 0_u64;
        // SAFETY: as in `load_item_definitions`; both out-parameters are
        // writable.
        let priced = unsafe {
            (client.lib.fns.inventory.get_item_price)(
                client.inventory,
                definition.0,
                &raw mut current,
                &raw mut base,
            )
        };
        priced.then_some(ItemPrice {
            definition,
            current,
            base,
        })
    }

    /// Makes one result-producing call. The handle Steam writes is owned
    /// before its answer is looked at, so a handle made for a call Steam then
    /// refused is destroyed on the way out.
    ///
    /// Every `call` passed here is made with the `Steam`'s live `inventory`
    /// on the pump thread — `Inventory` borrows the `!Send` `Steam` — and a
    /// writable handle.
    fn operation(
        &self,
        name: &'static str,
        call: impl FnOnce(&Client, *mut SteamInventoryResult) -> bool,
    ) -> Result<InventoryResult, SteamError> {
        let client = &self.steam.client;
        let mut handle = INVALID_RESULT;
        let started = call(client, &raw mut handle);
        let result = (handle != INVALID_RESULT).then(|| InventoryResult {
            client: Arc::clone(client),
            handle,
            _not_send: PhantomData,
        });
        match (started, result) {
            (true, Some(result)) => Ok(result),
            _ => Err(SteamError::Refused(name)),
        }
    }
}

/// A length as the `uint32` count Steam takes, refused past it.
fn count(len: usize, what: &'static str) -> Result<u32, SteamError> {
    u32::try_from(len).map_err(|_| SteamError::TooMany {
        what,
        max: u32::MAX as usize,
    })
}

/// An inventory operation's result (`SteamInventoryResult_t`): pending until
/// [`SteamEvent::InventoryResultReady`](crate::SteamEvent::InventoryResultReady)
/// names it, then the items it holds. Destroyed (`DestroyResult`) exactly
/// once, when dropped.
#[derive(Debug)]
pub struct InventoryResult {
    client: Arc<Client>,
    handle: SteamInventoryResult,
    _not_send: PhantomData<*const ()>,
}

impl InventoryResult {
    /// Its handle, as the ready event names it.
    #[must_use]
    pub const fn id(&self) -> InventoryResultId {
        InventoryResultId(self.handle)
    }

    /// Where it stands (`GetResultStatus`): [`EResult::PENDING`] until it is
    /// ready, then `EResult::OK` or why it failed.
    #[must_use]
    pub fn status(&self) -> EResult {
        let client = &self.client;
        // SAFETY: `client.inventory` is live; `InventoryResult` is `!Send`,
        // so this is the pump thread; the handle is this value's, not yet
        // destroyed.
        EResult(unsafe {
            (client.lib.fns.inventory.get_result_status)(client.inventory, self.handle)
        })
    }

    /// The items it holds (`GetResultItems`, asked for the count, then the
    /// items).
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam will not hand them over — a result
    /// still pending, or one that failed.
    pub fn items(&self) -> Result<Vec<InventoryItem>, SteamError> {
        let client = &self.client;
        let mut count = 0_u32;
        // SAFETY: as in `status`; a null array asks for the count, written to
        // the writable `count`.
        let known = unsafe {
            (client.lib.fns.inventory.get_result_items)(
                client.inventory,
                self.handle,
                core::ptr::null_mut(),
                &raw mut count,
            )
        };
        if !known {
            return Err(SteamError::Refused("GetResultItems"));
        }
        let empty = structs::SteamItemDetails {
            item: 0,
            definition: 0,
            quantity: 0,
            flags: 0,
        };
        let mut items = vec![empty; usize::try_from(count).unwrap_or(0)];
        let mut written = u32::try_from(items.len()).unwrap_or(0);
        // SAFETY: as in `status`; `items` is `written` writable
        // `SteamItemDetails_t`s, and `written` is writable.
        let read = unsafe {
            (client.lib.fns.inventory.get_result_items)(
                client.inventory,
                self.handle,
                items.as_mut_ptr(),
                &raw mut written,
            )
        };
        if !read {
            return Err(SteamError::Refused("GetResultItems"));
        }
        // Never past the buffer, whatever Steam's count says.
        items.truncate(usize::try_from(written).unwrap_or(0));
        Ok(items
            .into_iter()
            .map(|raw| InventoryItem {
                id: ItemInstanceId(raw.item),
                definition: ItemDef(raw.definition),
                quantity: raw.quantity,
                flags: ItemFlags(raw.flags),
            })
            .collect())
    }

    /// When Steam's servers made it, in Unix seconds
    /// (`GetResultTimestamp`); zero while it is pending.
    #[must_use]
    pub fn timestamp(&self) -> u32 {
        let client = &self.client;
        // SAFETY: as in `status`.
        unsafe { (client.lib.fns.inventory.get_result_timestamp)(client.inventory, self.handle) }
    }

    /// Whether it describes `user`'s inventory (`CheckResultSteamID`) — for
    /// a result another player sent.
    #[must_use]
    pub fn belongs_to(&self, user: SteamId) -> bool {
        let client = &self.client;
        // SAFETY: as in `status`.
        unsafe {
            (client.lib.fns.inventory.check_result_steam_id)(client.inventory, self.handle, user.0)
        }
    }
}

impl Drop for InventoryResult {
    fn drop(&mut self) {
        let client = &self.client;
        // SAFETY: as in `status`; the handle is destroyed here once.
        unsafe { (client.lib.fns.inventory.destroy_result)(client.inventory, self.handle) };
    }
}

/// The answer to [`Inventory::start_purchase`]
/// (`SteamInventoryStartPurchaseResult_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PurchaseStarted {
    /// `EResult::OK` when the checkout opened, or why not.
    pub result: EResult,
    /// The order's id.
    pub order: u64,
    /// The transaction's id.
    pub transaction: u64,
}

impl Answer for PurchaseStarted {
    const ROW: CallRow = CallRow {
        base: Base::Inventory,
        offset: 4,
        #[cfg(test)]
        name: "SteamInventoryStartPurchaseResult_t",
        size: size_of::<structs::SteamInventoryStartPurchaseResult>(),
    };

    fn build(bytes: &[u8], _: &mut Steam) -> Option<Self> {
        let raw = read::<structs::SteamInventoryStartPurchaseResult>(bytes)?;
        Some(Self {
            result: EResult(raw.result),
            order: raw.order,
            transaction: raw.transaction,
        })
    }

    fn abandon(_: &[u8], _: &Client) {}
}

/// The answer to [`Inventory::request_prices`]
/// (`SteamInventoryRequestPricesResult_t`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PricesReady {
    /// `EResult::OK` when the prices can be read, or why not.
    pub result: EResult,
    /// The ISO 4217 code of every price, e.g. `USD`.
    pub currency: String,
}

impl Answer for PricesReady {
    const ROW: CallRow = CallRow {
        base: Base::Inventory,
        offset: 5,
        #[cfg(test)]
        name: "SteamInventoryRequestPricesResult_t",
        size: size_of::<structs::SteamInventoryRequestPricesResult>(),
    };

    fn build(bytes: &[u8], steam: &mut Steam) -> Option<Self> {
        let raw = read::<structs::SteamInventoryRequestPricesResult>(bytes)?;
        let (currency, lossy) = fixed_string(&{ raw.currency });
        if lossy {
            steam.lossy_strings.set(steam.lossy_strings.get() + 1);
        }
        Some(Self {
            result: EResult(raw.result),
            currency,
        })
    }

    fn abandon(_: &[u8], _: &Client) {}
}

#[cfg(test)]
mod tests;
