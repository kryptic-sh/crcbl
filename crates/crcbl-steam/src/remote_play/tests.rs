//! Remote Play over the fake.

use super::*;
use crate::{
    AppId, SteamEvent,
    client::init_on,
    testing::{self, FakeMsg, FakeSession},
};

fn steam() -> Steam {
    init_on(testing::fake_lib(), AppId(480)).unwrap()
}

fn sessions(list: Vec<FakeSession>) {
    testing::script(|s| s.remote_play.sessions = list);
}

#[test]
fn sessions_and_what_each_is_are_steams() {
    let steam = steam();
    assert!(steam.remote_play().sessions().is_empty());
    sessions(vec![
        FakeSession {
            id: 3,
            together: true,
            user: 76_561_197_960_287_931,
            guest: 12,
            name: Some(c"Living-room TV"),
            form_factor: 4,
            resolution: Some((1920, 1080)),
        },
        FakeSession {
            id: 4,
            ..FakeSession::default()
        },
    ]);
    let play = steam.remote_play();
    assert_eq!(
        play.sessions(),
        [RemotePlaySession(3), RemotePlaySession(4)]
    );
    let tv = RemotePlaySession(3);
    assert!(play.together(tv));
    assert_eq!(play.user(tv), SteamId(76_561_197_960_287_931));
    assert_eq!(play.guest(tv), Some(12));
    assert_eq!(play.client_name(tv), Some("Living-room TV".to_owned()));
    assert_eq!(play.form_factor(tv), FormFactor::Tv);
    assert_eq!(play.resolution(tv), Some((1920, 1080)));

    let bare = RemotePlaySession(4);
    assert!(!play.together(bare));
    assert_eq!(play.guest(bare), None, "not a guest");
    assert_eq!(play.client_name(bare), None, "Steam answered null");
    assert_eq!(play.resolution(bare), None);
    assert_eq!(
        steam.diagnostics().lossy_strings,
        0,
        "a null here is not lossy"
    );
}

/// Zero by zero is Steam's "not known", as a `false` is.
#[test]
fn an_unknown_resolution_is_none() {
    let steam = steam();
    sessions(vec![FakeSession {
        id: 5,
        resolution: Some((0, 0)),
        ..FakeSession::default()
    }]);
    assert_eq!(steam.remote_play().resolution(RemotePlaySession(5)), None);
}

#[test]
fn form_factors_map_and_an_unnamed_one_is_kept() {
    for (raw, factor) in [
        (0, FormFactor::Unknown),
        (1, FormFactor::Phone),
        (2, FormFactor::Tablet),
        (3, FormFactor::Computer),
        (4, FormFactor::Tv),
        (5, FormFactor::VrHeadset),
        (9, FormFactor::Other(9)),
    ] {
        assert_eq!(FormFactor::from_raw(raw), factor, "{raw}");
    }
}

#[test]
fn invites_and_the_panel_reach_steam_and_a_refusal_is_an_error() {
    let steam = steam();
    let play = steam.remote_play();
    play.invite(SteamId(9)).unwrap();
    play.show_together_panel().unwrap();
    assert_eq!(testing::script(|s| s.remote_play.invites.clone()), [9]);
    assert_eq!(testing::script(|s| s.remote_play.panels), 1);
    testing::script(|s| s.refuse = true);
    assert_eq!(
        play.invite(SteamId(9)),
        Err(SteamError::Refused("BSendRemotePlayTogetherInvite"))
    );
    assert_eq!(
        play.show_together_panel(),
        Err(SteamError::Refused("ShowRemotePlayTogetherUI"))
    );
}

#[test]
fn sessions_coming_and_going_are_events() {
    let mut steam = steam();
    testing::script(|s| {
        s.queue
            .push_back(FakeMsg::payload(5701, 3_u32.to_le_bytes().to_vec()));
        s.queue
            .push_back(FakeMsg::payload(5702, 3_u32.to_le_bytes().to_vec()));
    });
    steam.pump();
    assert_eq!(
        steam.events().collect::<Vec<_>>(),
        [
            SteamEvent::RemotePlayConnected {
                session: RemotePlaySession(3)
            },
            SteamEvent::RemotePlayDisconnected {
                session: RemotePlaySession(3)
            },
        ]
    );
}
