// The client and the website ship separately, so a page the browser cached before a deeplink field
// existed still has to open the app. These pin down that the play link keeps working without the
// thumbnail uuid, and reads it when it is there.

use client::parse_client_mode::{DeepLinkParams, DeepLinkParseError, parse_deep_link};

fn parse(deep_link: &str) -> DeepLinkParams {
    parse_deep_link(&url::Url::parse(deep_link).expect("the test deep link should be a url")).unwrap_or_else(|_| panic!("{deep_link} did not parse"))
}

#[test]
fn a_play_link_carrying_a_thumbnail_uuid_reads_it() {
    let DeepLinkParams::Play {
        request_id,
        play_auth,
        thumbnail_uuid,
        ..
    } = parse("kube-new://?mode=play&request_id=abc&play_auth=secret&thumbnail_uuid=00000000-0000-0000-0000-000000004242")
    else {
        panic!("a play link did not parse as play mode");
    };
    assert_eq!(request_id, "abc");
    assert_eq!(play_auth, "secret");
    assert_eq!(thumbnail_uuid, Some(uuid::Uuid::from_u128(0x4242)));
}

// The loading screen shows the game's name, so it travels on the link the same way the thumbnail
// does, and links that predate it still open on the plain wait.
#[test]
fn a_play_link_carrying_a_thumbnail_game_name_reads_it() {
    let DeepLinkParams::Play { thumbnail_game_name, .. } = parse("kube-new://?mode=play&request_id=abc&play_auth=secret&thumbnail_game_name=Sky%20Racer%21")
    else {
        panic!("a play link with a thumbnail game name did not parse as play mode");
    };
    assert_eq!(thumbnail_game_name.as_deref(), Some("Sky Racer!"));
}

#[test]
fn a_play_link_from_before_the_thumbnail_game_name_existed_still_opens() {
    let DeepLinkParams::Play { thumbnail_game_name, .. } = parse("kube-new://?mode=play&request_id=abc&play_auth=secret") else {
        panic!("a play link without a thumbnail game name did not parse as play mode");
    };
    assert_eq!(thumbnail_game_name, None);
}

#[test]
fn a_play_link_from_before_the_thumbnail_uuid_existed_still_opens() {
    let DeepLinkParams::Play {
        request_id, thumbnail_uuid, ..
    } = parse("kube-new://?mode=play&request_id=abc&play_auth=secret")
    else {
        panic!("a play link without a thumbnail uuid did not parse as play mode");
    };
    assert_eq!(request_id, "abc");
    assert_eq!(thumbnail_uuid, None);
}

#[test]
fn a_play_link_whose_thumbnail_uuid_is_unreadable_still_opens() {
    let DeepLinkParams::Play { thumbnail_uuid, .. } = parse("kube-new://?mode=play&request_id=abc&play_auth=secret&thumbnail_uuid=not-a-uuid") else {
        panic!("a play link with an unreadable thumbnail uuid did not parse as play mode");
    };
    assert_eq!(thumbnail_uuid, None);
}

#[test]
fn an_unknown_mode_is_still_refused() {
    let unknown_mode_result = parse_deep_link(&url::Url::parse("kube-new://?mode=unknown").expect("the test deep link should be a url"));
    assert!(unknown_mode_result.is_err(), "an unknown mode parsed as something");
}

// Removed editor and browser-recovery links must not start a desktop action.
#[test]
fn removed_link_modes_are_refused() {
    for mode in ["edit", "prompt", "see-browser"] {
        let deep_link = url::Url::parse(&format!(
            "kube-new://?mode={mode}&game_id=42&edit_auth=secret&prompt=Make+a+game&attachment_url=https%3A%2F%2Fexample.com%2Fimage"
        ))
        .expect("the test deep link should be a url");
        match parse_deep_link(&deep_link) {
            Err(DeepLinkParseError::InvalidMode) => {}
            _ => panic!("{mode} must no longer be accepted"),
        }
    }
}

#[test]
fn a_blank_link_still_returns_to_the_browser() {
    let DeepLinkParams::Blank = parse("kube-new://?mode=blank") else {
        panic!("blank mode should open the homepage");
    };
}

#[test]
fn play_requires_authorization_and_a_server() {
    for (query, expected_field) in [("mode=play&request_id=abc", "play_auth"), ("mode=play&play_auth=secret", "request_id")] {
        let deep_link = url::Url::parse(&format!("kube-new://?{query}")).expect("the test deep link should be a url");
        match parse_deep_link(&deep_link) {
            Err(DeepLinkParseError::MissingField(field)) => assert_eq!(field, expected_field),
            _ => panic!("play link should require {expected_field}"),
        }
    }
}
