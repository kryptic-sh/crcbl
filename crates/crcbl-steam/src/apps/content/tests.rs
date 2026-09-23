//! Ownership, DLC, betas and the install directory over the fake.

use super::*;
use crate::{
    CallState, Steam, SteamEvent,
    client::init_on,
    testing::{self, FakeApps, FakeBeta, FakeMsg, completion},
};

fn steam() -> Steam {
    init_on(testing::fake_lib(), AppId(480)).unwrap()
}

fn fake<R>(f: impl FnOnce(&mut FakeApps) -> R) -> R {
    testing::script(|s| f(&mut s.apps_extra))
}

fn offered(call: &str) -> Vec<usize> {
    fake(|f| {
        f.offered
            .iter()
            .filter(|(name, _)| *name == call)
            .map(|&(_, size)| size)
            .collect()
    })
}

#[test]
fn ownership_answers_are_steams() {
    let steam = steam();
    let apps = steam.apps();
    assert!(!apps.low_violence() && !apps.vac_banned());
    assert!(!apps.free_weekend() && !apps.family_shared());
    fake(|f| {
        f.yes = vec![
            "low violence",
            "vac banned",
            "free weekend",
            "family shared",
        ];
        f.owned_apps = vec![481];
        f.purchase_time = 1_700_000_000;
        f.owner = 42;
        f.build = 9_001;
    });
    assert!(apps.low_violence() && apps.vac_banned());
    assert!(apps.free_weekend() && apps.family_shared());
    assert!(apps.subscribed_app(AppId(481)));
    assert!(!apps.subscribed_app(AppId(482)));
    assert_eq!(apps.purchase_time(AppId(481)), 1_700_000_000);
    assert_eq!(apps.owner(), SteamId(42));
    assert_eq!(apps.build_id(), 9_001);
}

/// **DLC by index decodes every field**, and an index Steam has no DLC at
/// is a refusal naming the call.
#[test]
fn dlc_by_index_decodes_and_a_missing_index_is_refused() {
    let steam = steam();
    fake(|f| {
        f.dlcs = vec![
            (1_001, true, b"Soundtrack".to_vec()),
            (1_002, false, b"Art Book".to_vec()),
        ];
        f.installed_dlc = vec![1_001];
    });
    let apps = steam.apps();
    assert_eq!(apps.dlc_count(), 2);
    assert_eq!(
        apps.dlc(1),
        Ok(Dlc {
            app: AppId(1_002),
            available: false,
            name: "Art Book".to_owned(),
        })
    );
    assert_eq!(apps.dlc(0).map(|dlc| dlc.available), Ok(true));
    assert_eq!(apps.dlc(2), Err(SteamError::Refused("BGetDLCDataByIndex")));
    assert!(apps.dlc_installed(AppId(1_001)));
    assert!(!apps.dlc_installed(AppId(1_002)));
    apps.install_dlc(AppId(1_002));
    apps.uninstall_dlc(AppId(1_001));
    assert_eq!(
        fake(|f| f.dlc_changes.clone()),
        [(true, 1_002), (false, 1_001)]
    );
}

/// **A name that fills the buffer is read again in a bigger one**: Steam's
/// copy stops a byte short for the NUL, so reaching it may be a cut.
#[test]
fn a_dlc_name_that_fills_the_buffer_grows_it_until_it_fits() {
    let steam = steam();
    let long = vec![b'n'; 300];
    fake(|f| f.dlcs = vec![(7, true, long.clone())]);
    let dlc = steam.apps().dlc(0).unwrap();
    assert_eq!(dlc.name.len(), 300, "the whole name");
    assert_eq!(offered("BGetDLCDataByIndex"), [256, 512]);

    // A name one byte short of the first buffer's NUL still grows it once,
    // since the cut cannot be told apart from a fit.
    fake(|f| {
        f.dlcs = vec![(7, true, vec![b'n'; 255])];
        f.offered.clear();
    });
    assert_eq!(steam.apps().dlc(0).map(|d| d.name.len()), Ok(255));
    assert_eq!(offered("BGetDLCDataByIndex"), [256, 512]);
    fake(|f| {
        f.dlcs = vec![(7, true, vec![b'n'; 254])];
        f.offered.clear();
    });
    assert_eq!(steam.apps().dlc(0).map(|d| d.name.len()), Ok(254));
    assert_eq!(
        offered("BGetDLCDataByIndex"),
        [256],
        "one read when it fits"
    );
}

/// A string that fills even the largest buffer is truncated, not cut.
#[test]
fn a_string_past_the_largest_buffer_is_truncated() {
    let steam = steam();
    fake(|f| f.install_dir = vec![b'd'; MAX_TEXT_BYTES]);
    assert_eq!(
        steam.apps().install_dir(AppId(480)),
        Err(SteamError::Truncated("GetAppInstallDir"))
    );
    let sizes = offered("GetAppInstallDir");
    assert_eq!(sizes.first(), Some(&256));
    assert_eq!(sizes.last(), Some(&MAX_TEXT_BYTES), "stopped at the cap");
}

#[test]
fn the_install_dir_grows_and_an_uninstalled_app_is_none() {
    let steam = steam();
    assert_eq!(steam.apps().install_dir(AppId(480)), Ok(None));
    let path = format!("C:/Games/{}", "deep/".repeat(80));
    fake(|f| f.install_dir = path.clone().into_bytes());
    assert_eq!(
        steam.apps().install_dir(AppId(480)),
        Ok(Some(PathBuf::from(&path)))
    );
    assert_eq!(offered("GetAppInstallDir").last(), Some(&512));
}

/// **Beta names and descriptions grow together**, both read whole.
#[test]
fn betas_decode_and_their_strings_grow() {
    let steam = steam();
    let description = "a".repeat(400);
    fake(|f| {
        f.betas = vec![
            FakeBeta {
                flags: 1 | 16,
                build: 10,
                name: b"public".to_vec(),
                description: b"Default".to_vec(),
                updated: 5,
            },
            FakeBeta {
                flags: 2 | 4,
                build: 11,
                name: b"experimental".to_vec(),
                description: description.clone().into_bytes(),
                updated: 6,
            },
        ];
        f.available_betas = 1;
        f.private_betas = 1;
        f.current_beta = Some(b"public".to_vec());
    });
    let apps = steam.apps();
    assert_eq!(
        apps.beta_count(),
        BetaCount {
            total: 2,
            available: 1,
            private: 1,
        }
    );
    let beta = apps.beta(1).unwrap();
    assert_eq!(beta.name, "experimental");
    assert_eq!(beta.description, description);
    assert!(beta.flags.contains(BetaFlags::PRIVATE));
    assert!(!beta.flags.contains(BetaFlags::SELECTED));
    assert_eq!((beta.build, beta.updated), (11, 6));
    assert_eq!(offered("GetBetaInfo"), [256, 256, 512, 512]);
    assert!(apps.beta(0).unwrap().flags.contains(BetaFlags::INSTALLED));
    assert_eq!(apps.beta(2), Err(SteamError::Refused("GetBetaInfo")));
    assert_eq!(apps.current_beta(), Ok("public".to_owned()));
    fake(|f| f.current_beta = None);
    assert_eq!(
        apps.current_beta(),
        Err(SteamError::Refused("GetCurrentBetaName"))
    );
}

#[test]
fn selecting_a_beta_and_marking_content_corrupt_reach_steam() {
    let steam = steam();
    let apps = steam.apps();
    apps.set_active_beta("experimental").unwrap();
    apps.mark_content_corrupt(true).unwrap();
    assert_eq!(
        fake(|f| f.active_beta.clone()),
        Some("experimental".to_owned())
    );
    assert_eq!(fake(|f| f.corrupt.clone()), [true]);
    testing::script(|s| s.refuse = true);
    assert_eq!(
        apps.set_active_beta("x"),
        Err(SteamError::Refused("SetActiveBeta"))
    );
    assert_eq!(
        apps.mark_content_corrupt(false),
        Err(SteamError::Refused("MarkContentCorrupt"))
    );
}

#[test]
fn file_details_answer_through_the_registry() {
    let mut steam = steam();
    assert!(matches!(
        steam.file_details("game.pak"),
        Err(SteamError::Refused("GetFileDetails"))
    ));
    fake(|f| f.file_call = 700);
    let call = steam.file_details("game.pak").unwrap();
    assert_eq!(fake(|f| f.file_asked.clone()), Some("game.pak".to_owned()));
    let size = size_of::<structs::FileDetailsResult>();
    let mut answer = vec![0_u8; size];
    answer[..4].copy_from_slice(&1_i32.to_le_bytes());
    // The size's offset follows the packing: 4 under `pack(4)`, 8 under
    // `pack(8)` — read off the declaration rather than assumed.
    let at = core::mem::offset_of!(structs::FileDetailsResult, size);
    answer[at..at + 8].copy_from_slice(&123_456_u64.to_le_bytes());
    let sha = core::mem::offset_of!(structs::FileDetailsResult, sha1);
    answer[sha..sha + 20].copy_from_slice(&[0xAB; 20]);
    testing::script(|s| {
        s.results.push((700, answer, false));
        s.queue.push_back(completion(700, 1023, size));
    });
    steam.pump();
    let CallState::Ready(details) = steam.take(call) else {
        panic!("answered");
    };
    assert_eq!(
        details,
        FileDetails {
            result: EResult::OK,
            size: 123_456,
            sha1: [0xAB; 20],
            flags: 0,
        }
    );
}

#[test]
fn an_installed_dlc_is_an_event() {
    let mut steam = steam();
    testing::script(|s| {
        s.queue
            .push_back(FakeMsg::payload(1005, 1_001_u32.to_le_bytes().to_vec()));
    });
    steam.pump();
    assert_eq!(
        steam.events().collect::<Vec<_>>(),
        [SteamEvent::DlcInstalled { app: AppId(1_001) }]
    );
}
