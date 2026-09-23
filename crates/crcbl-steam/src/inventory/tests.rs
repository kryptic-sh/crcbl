//! The inventory over the fake: result handles, items, definitions, prices.

use super::*;
use crate::{
    AppId, CallResult, CallState, SteamEvent,
    call::private::Answer,
    client::init_on,
    testing::{self, FakeInventory, FakeMsg, completion, payload},
};

fn steam() -> Steam {
    init_on(testing::fake_lib(), AppId(480)).unwrap()
}

fn fake<R>(f: impl FnOnce(&mut FakeInventory) -> R) -> R {
    testing::script(|s| f(&mut s.inventory))
}

/// Every logged call whose name is `name`.
fn logged(name: &str) -> Vec<String> {
    fake(|f| {
        f.log
            .iter()
            .filter(|line| *line == name || line.starts_with(&format!("{name}|")))
            .cloned()
            .collect()
    })
}

fn item(id: u64, definition: i32, quantity: u16, flags: u16) -> structs::SteamItemDetails {
    structs::SteamItemDetails {
        item: id,
        definition,
        quantity,
        flags,
    }
}

/// Pumps a completion of call 77 with `bytes`, answered with `T`'s row.
fn answer<T: CallResult>(steam: &mut Steam, bytes: Vec<u8>) {
    let row = <T as Answer>::ROW;
    testing::script(|s| {
        s.results.retain(|(call, ..)| *call != 77);
        s.results.push((77, bytes, false));
        s.queue.push_back(completion(77, row.id(), row.size));
    });
    steam.pump();
}

fn ready<T: CallResult + core::fmt::Debug>(steam: &mut Steam, call: SteamCall<T>) -> T {
    match steam.take(call) {
        CallState::Ready(answer) => answer,
        other => panic!("not answered: {other:?}"),
    }
}

/// **Result-handle RAII**: a result is destroyed once when dropped, whether
/// or not it became ready, and never before.
#[test]
fn a_result_is_destroyed_once_when_dropped_and_not_before() {
    let mut steam = steam();
    fake(|f| f.handle = 11);
    let result = steam.inventory().all_items().unwrap();
    assert_eq!(result.id(), InventoryResultId(11));
    fake(|f| f.status = 22);
    assert_eq!(result.status(), EResult::PENDING);
    assert!(logged("DestroyResult").is_empty());
    drop(result);
    assert_eq!(logged("DestroyResult"), ["DestroyResult|11"]);
}

/// A handle Steam wrote for an operation it then refused is destroyed, not
/// leaked; with no handle written, nothing is destroyed.
#[test]
fn a_refused_operation_destroys_the_handle_steam_made() {
    let mut steam = steam();
    fake(|f| {
        f.handle = 12;
        f.refuse = true;
    });
    assert_eq!(
        steam.inventory().all_items().unwrap_err(),
        SteamError::Refused("GetAllItems")
    );
    assert_eq!(logged("DestroyResult"), ["DestroyResult|12"]);
    fake(|f| {
        f.log.clear();
        f.handle = -1;
    });
    assert_eq!(
        steam.inventory().grant_promo_items().unwrap_err(),
        SteamError::Refused("GrantPromoItems")
    );
    fake(|f| f.refuse = false);
    assert_eq!(
        steam.inventory().grant_promo_items().unwrap_err(),
        SteamError::Refused("GrantPromoItems"),
        "started, but no handle to own"
    );
    assert!(logged("DestroyResult").is_empty());
}

#[test]
fn each_operation_passes_its_arguments() {
    let mut steam = steam();
    fake(|f| f.handle = 3);
    let inventory = steam.inventory();
    drop(
        inventory
            .items_by_id(&[ItemInstanceId(7), ItemInstanceId(8)])
            .unwrap(),
    );
    drop(inventory.grant_promo_items().unwrap());
    drop(inventory.add_promo_item(ItemDef(100)).unwrap());
    drop(inventory.consume(ItemInstanceId(7), 2).unwrap());
    drop(
        inventory
            .exchange(
                &[(ItemDef(200), 1)],
                &[(ItemInstanceId(7), 1), (ItemInstanceId(8), 3)],
            )
            .unwrap(),
    );
    drop(inventory.exchange(&[], &[]).unwrap());
    let calls: Vec<String> = fake(|f| {
        f.log
            .iter()
            .filter(|line| !line.starts_with("DestroyResult"))
            .cloned()
            .collect()
    });
    assert_eq!(
        calls,
        [
            "GetItemsByID|7,8",
            "GrantPromoItems",
            "AddPromoItem|100",
            "ConsumeItem|7|2",
            "ExchangeItems|200|1|7,8|1,3",
            "ExchangeItems||||",
        ]
    );
    assert_eq!(logged("DestroyResult").len(), 6, "each result once");
}

#[test]
fn consuming_nothing_is_refused_before_the_call() {
    let mut steam = steam();
    assert_eq!(
        steam.inventory().consume(ItemInstanceId(7), 0).unwrap_err(),
        SteamError::OutOfRange("quantity")
    );
    assert!(fake(|f| f.log.is_empty()));
}

#[test]
fn a_result_reads_its_items_timestamp_and_owner() {
    let mut steam = steam();
    fake(|f| {
        f.handle = 11;
        f.items = vec![item(1, 100, 1, 0), item(2, 101, 5, 1 << 9)];
    });
    let result = steam.inventory().all_items().unwrap();
    let items = result.items().unwrap();
    assert_eq!(
        items,
        [
            InventoryItem {
                id: ItemInstanceId(1),
                definition: ItemDef(100),
                quantity: 1,
                flags: ItemFlags(0),
            },
            InventoryItem {
                id: ItemInstanceId(2),
                definition: ItemDef(101),
                quantity: 5,
                flags: ItemFlags::CONSUMED,
            },
        ]
    );
    assert!(items[1].flags.contains(ItemFlags::CONSUMED));
    assert!(!items[1].flags.contains(ItemFlags::NO_TRADE));
    assert_eq!(
        logged("GetResultItems"),
        ["GetResultItems|11|count", "GetResultItems|11|2"]
    );
    assert_eq!(result.timestamp(), 1_700_000_000);
    assert!(result.belongs_to(SteamId(testing::STEAM_ID)));
    assert!(!result.belongs_to(SteamId(1)));
}

/// However many items Steam claims, no more are read than the buffer held.
#[test]
fn result_items_never_read_past_their_buffer() {
    let mut steam = steam();
    fake(|f| {
        f.handle = 11;
        f.items = vec![item(1, 1, 1, 0), item(2, 2, 1, 0), item(3, 3, 1, 0)];
        f.claimed = Some(1);
    });
    let result = steam.inventory().all_items().unwrap();
    assert_eq!(result.items().unwrap().len(), 1);
    fake(|f| f.claimed = Some(5));
    assert_eq!(result.items().unwrap().len(), 3, "as many as were written");
}

#[test]
fn definitions_are_refused_until_loaded_then_listed() {
    let mut steam = steam();
    assert_eq!(
        steam.inventory().item_definitions(),
        Err(SteamError::Refused("GetItemDefinitionIDs"))
    );
    // Refused at the count: the ids are never asked for.
    assert_eq!(
        logged("GetItemDefinitionIDs"),
        ["GetItemDefinitionIDs|count"]
    );
    steam.inventory().load_item_definitions().unwrap();
    fake(|f| f.definitions = Some(vec![100, 101, 200]));
    assert_eq!(
        steam.inventory().item_definitions(),
        Ok(vec![ItemDef(100), ItemDef(101), ItemDef(200)])
    );
    testing::script(|s| s.refuse = true);
    assert_eq!(
        steam.inventory().load_item_definitions(),
        Err(SteamError::Refused("LoadItemDefinitions"))
    );
}

/// A property is read into a buffer grown until it fits, and a property
/// Steam does not have is refused.
#[test]
fn a_definition_property_grows_until_it_fits() {
    let mut steam = steam();
    assert_eq!(
        steam
            .inventory()
            .definition_property(ItemDef(100), Some("name")),
        Err(SteamError::Refused("GetItemDefinitionProperty"))
    );
    fake(|f| {
        f.property_offered.clear();
        f.property = Some(vec![b'n'; 300]);
    });
    assert_eq!(
        steam
            .inventory()
            .definition_property(ItemDef(100), Some("name")),
        Ok("n".repeat(300))
    );
    assert_eq!(fake(|f| f.property_offered.clone()), [256, 512]);
    fake(|f| f.property = Some(b"name,description,price".to_vec()));
    assert_eq!(
        steam.inventory().definition_property(ItemDef(100), None),
        Ok("name,description,price".into())
    );
    assert_eq!(
        logged("GetItemDefinitionProperty")[..2],
        [
            "GetItemDefinitionProperty|100|name",
            "GetItemDefinitionProperty|100|name"
        ]
    );
    assert!(
        logged("GetItemDefinitionProperty")
            .last()
            .is_some_and(|line| line.ends_with("|(names)"))
    );
    assert_eq!(
        steam
            .inventory()
            .definition_property(ItemDef(100), Some("a\0b")),
        Err(SteamError::InteriorNul("property"))
    );
}

#[test]
fn a_purchase_and_prices_answer_their_calls() {
    use core::mem::offset_of;
    use structs::{SteamInventoryRequestPricesResult as P, SteamInventoryStartPurchaseResult as S};
    let mut steam = steam();
    fake(|f| f.call = 77);

    let call = steam
        .inventory()
        .start_purchase(&[(ItemDef(100), 2), (ItemDef(101), 1)])
        .unwrap();
    assert_eq!(logged("StartPurchase"), ["StartPurchase|100,101|2,1"]);
    answer::<PurchaseStarted>(
        &mut steam,
        payload::<S>(&[
            (offset_of!(S, result), &1_i32.to_le_bytes()),
            (offset_of!(S, order), &55_u64.to_le_bytes()),
            (offset_of!(S, transaction), &66_u64.to_le_bytes()),
        ]),
    );
    assert_eq!(
        ready(&mut steam, call),
        PurchaseStarted {
            result: EResult::OK,
            order: 55,
            transaction: 66,
        }
    );

    let call = steam.inventory().request_prices().unwrap();
    answer::<PricesReady>(
        &mut steam,
        payload::<P>(&[
            (offset_of!(P, result), &1_i32.to_le_bytes()),
            (offset_of!(P, currency), b"USD\0"),
        ]),
    );
    assert_eq!(
        ready(&mut steam, call),
        PricesReady {
            result: EResult::OK,
            currency: "USD".into(),
        }
    );
    fake(|f| f.prices = vec![(100, 199, 299), (101, 99, 99)]);
    assert_eq!(
        steam.inventory().prices(),
        Ok(vec![
            ItemPrice {
                definition: ItemDef(100),
                current: 199,
                base: 299,
            },
            ItemPrice {
                definition: ItemDef(101),
                current: 99,
                base: 99,
            },
        ])
    );
    assert_eq!(
        steam.inventory().price(ItemDef(101)),
        Some(ItemPrice {
            definition: ItemDef(101),
            current: 99,
            base: 99,
        })
    );
    assert_eq!(steam.inventory().price(ItemDef(7)), None);
}

#[test]
fn a_call_steam_does_not_start_is_refused_naming_it() {
    let mut steam = steam();
    assert_eq!(
        steam.inventory().start_purchase(&[]).unwrap_err(),
        SteamError::Refused("StartPurchase")
    );
    assert_eq!(
        steam.inventory().request_prices().unwrap_err(),
        SteamError::Refused("RequestPrices")
    );
}

#[test]
fn ready_results_full_updates_and_definition_changes_arrive_as_events() {
    let mut steam = steam();
    let mut ready_bytes = 11_i32.to_le_bytes().to_vec();
    ready_bytes.extend_from_slice(&1_i32.to_le_bytes());
    testing::script(|s| {
        s.queue
            .push_back(FakeMsg::payload(4701, 11_i32.to_le_bytes().to_vec()));
        s.queue.push_back(FakeMsg::payload(4700, ready_bytes));
        s.queue.push_back(FakeMsg::payload(4702, vec![0]));
    });
    steam.pump();
    assert_eq!(
        steam.events().collect::<Vec<_>>(),
        [
            SteamEvent::InventoryFullUpdate {
                result: InventoryResultId(11),
            },
            SteamEvent::InventoryResultReady {
                result: InventoryResultId(11),
                status: EResult::OK,
            },
            SteamEvent::InventoryDefinitionsUpdated,
        ]
    );
    assert_eq!(steam.diagnostics().decode_mismatches, 0);
}

/// Each answer and event under its header id — spelled out, since the call
/// tests build their completions from the row they check.
#[test]
fn the_inventory_answers_and_events_have_valves_ids() {
    assert_eq!(<PurchaseStarted as Answer>::ROW.id(), 4704);
    assert_eq!(<PricesReady as Answer>::ROW.id(), 4705);
    for (id, name) in [
        (4700, "SteamInventoryResultReady_t"),
        (4701, "SteamInventoryFullUpdate_t"),
        (4702, "SteamInventoryDefinitionUpdate_t"),
    ] {
        assert_eq!(
            crate::callbacks::find(id).map(|row| row.name),
            Some(name),
            "{id}"
        );
    }
}
