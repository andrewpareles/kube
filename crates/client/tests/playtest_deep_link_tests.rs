use client::deep_link_receiver::find_deeplink_in_args_list;
use client::parse_client_mode::{DeepLinkParams, parse_deep_link};
#[test]
fn playtest_links_keep_their_own_join_credentials() {
    for ip in ["203.0.113.5", "2001:db8::5"] {
        let url = url::Url::parse(&format!("kube-new://?mode=playtest&public_ip={ip}&port=5000&join_token=temporary")).unwrap();
        let parsed = parse_deep_link(&url).ok().unwrap();
        let DeepLinkParams::Playtest { public_ip, port, join_token } = parsed else {
            panic!("Expected playtest link")
        };
        assert_eq!(public_ip, ip);
        assert_eq!(port, 5000);
        assert_eq!(join_token, "temporary");
    }
}
#[test]
fn play_credentials_cannot_be_used_as_playtest_credentials() {
    for query in [
        "mode=playtest&public_ip=203.0.113.5&port=5000&play_auth=play",
        "mode=playtest&public_ip=203.0.113.5&port=5000&join_token=",
        "mode=edit&request_id=deployment",
    ] {
        assert!(parse_deep_link(&url::Url::parse(&format!("kube-new://?{query}")).unwrap()).is_err());
    }
}

#[test]
fn playtest_links_require_a_server_address_and_valid_port() {
    for query in [
        "mode=playtest&request_id=deployment&join_token=temporary",
        "mode=playtest&public_ip=&port=5000&join_token=temporary",
        "mode=playtest&public_ip=203.0.113.5&join_token=temporary",
        "mode=playtest&public_ip=203.0.113.5&port=0&join_token=temporary",
        "mode=playtest&public_ip=203.0.113.5&port=-1&join_token=temporary",
        "mode=playtest&public_ip=203.0.113.5&port=65536&join_token=temporary",
        "mode=playtest&public_ip=203.0.113.5&port=invalid&join_token=temporary",
    ] {
        assert!(parse_deep_link(&url::Url::parse(&format!("kube-new://?{query}")).unwrap()).is_err(), "{query}");
    }
}

#[test]
fn a_spawned_player_reads_the_same_playtest_link_from_its_arguments() {
    let mut link = url::Url::parse("kube-new://").unwrap();
    link.query_pairs_mut()
        .append_pair("mode", "playtest")
        .append_pair("public_ip", "203.0.113.5")
        .append_pair("port", "5000")
        .append_pair("join_token", "token+with&reserved=characters");
    let launch_link = find_deeplink_in_args_list(["--open-separately".to_string(), link.as_str().to_string()].into_iter()).unwrap();
    let DeepLinkParams::Playtest { public_ip, port, join_token } = parse_deep_link(&launch_link).ok().unwrap() else {
        panic!("Expected the child process to receive a playtest link")
    };
    assert_eq!(public_ip, "203.0.113.5");
    assert_eq!(port, 5000);
    assert_eq!(join_token, "token+with&reserved=characters");
}

#[test]
fn launch_arguments_ignore_other_schemes_and_keep_normal_play_links() {
    let args = [
        "unrelated-argument",
        "https://example.com/?mode=playtest",
        "kube-new://?mode=play&request_id=deployment&play_auth=play",
    ];
    let link = find_deeplink_in_args_list(args.into_iter().map(str::to_string)).unwrap();
    let DeepLinkParams::Play { request_id, play_auth, .. } = parse_deep_link(&link).ok().unwrap() else {
        panic!("Normal Play must retain its primary app routing")
    };
    assert_eq!(request_id, "deployment");
    assert_eq!(play_auth, "play");
}

#[test]
fn a_blank_launch_has_no_deep_link() {
    assert!(find_deeplink_in_args_list(std::iter::empty()).is_none());
}
