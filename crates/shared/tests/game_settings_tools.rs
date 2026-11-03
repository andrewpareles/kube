use serde_json::json;
use shared::api::protocol_for_chat::{GetGameSettingsArgs, SetGameSettingsArgs};

#[test]
fn invalid_settings_are_rejected_before_database_access() {
    for body in [
        json!({}),
        json!({"title":null,"description":null,"suggested_min_players":null,"max_players":null,"visibility":null}),
        json!({"title":" \n"}),
        json!({"suggested_min_players":0}),
        json!({"suggested_min_players":-1}),
        json!({"suggested_min_players":i64::from(i32::MAX) + 1}),
        json!({"suggested_min_players":3,"max_players":2}),
        json!({"max_players":0}),
        json!({"max_players":-1}),
        json!({"max_players":i64::from(i32::MAX) + 1}),
        json!({"game_id":123}),
    ] {
        let mut args: SetGameSettingsArgs = serde_json::from_value(body.clone()).unwrap();
        assert!(args.validate().is_err(), "{body}");
    }
    for body in [json!({"suggested_min_players":1.5}), json!({"max_players":1.5}), json!({"visibility":"invalid"}), json!({"title":123})] {
        assert!(serde_json::from_value::<SetGameSettingsArgs>(body).is_err());
    }
}

#[test]
fn partial_updates_trim_text_and_keep_omitted_fields_unchanged() {
    let mut args: SetGameSettingsArgs = serde_json::from_value(json!({"title":" My game ","game_id":123,"creator_id":"other"})).unwrap();
    args.validate().unwrap();
    assert_eq!(
        serde_json::to_value(args).unwrap(),
        json!({"title":"My game","description":null,"suggested_min_players":null,"max_players":null,"visibility":null})
    );
    // Neither settings tool accepts a game identity; the editor supplies its own game id.
    let args: GetGameSettingsArgs = serde_json::from_value(json!({"game_id":123})).unwrap();
    assert_eq!(serde_json::to_value(args).unwrap(), json!({}));
}

#[test]
fn empty_description_clears_it_while_null_leaves_it_unchanged() {
    for description in ["", " \n\t"] {
        let mut args: SetGameSettingsArgs = serde_json::from_value(json!({"description":description})).unwrap();
        args.validate().unwrap();
        assert_eq!(args.description.as_deref(), Some(""));
    }
    let mut args: SetGameSettingsArgs = serde_json::from_value(json!({"title":"Game","description":null})).unwrap();
    args.validate().unwrap();
    assert!(args.description.is_none());
}

#[test]
fn visibility_and_player_limits_keep_the_existing_tool_format() {
    for visibility in ["Private", "Published"] {
        for max_players in [1, i32::MAX] {
            let mut args: SetGameSettingsArgs =
                serde_json::from_value(json!({"suggested_min_players":1,"max_players":max_players,"visibility":visibility})).unwrap();
            args.validate().unwrap();
            assert_eq!(
                serde_json::to_value(args).unwrap(),
                json!({"title":null,"description":null,"suggested_min_players":1,"max_players":max_players,"visibility":visibility})
            );
        }
    }
}
