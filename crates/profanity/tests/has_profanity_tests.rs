use profanity::{censor, has_profanity, has_profanity_in_asset_name};

// Every one of these was refused as an asset name: digits read as letters across the gaps between
// words, or a word in capitals counted as shouting.
#[test]
fn common_file_names_are_not_read_as_bad_words() {
    for name in [
        "Water_001_COLOR",
        "Water_001_METAL",
        "Fabric081C_1K-PNG_Color",
        "Fabric081C 1K Color",
        "Metal044A_2K-PNG_NormalGL",
        "Metal060B_1K-PNG_NormalGL",
        "IMG_20240101_123456",
        "PXL_20230815_101112345",
        "Photo_20240101_123456",
        "TEXTURE_ATLAS",
    ] {
        assert!(!has_profanity_in_asset_name(name), "{name} was refused");
    }
}

#[test]
fn bad_words_are_still_caught_through_digits_separators_and_capitals() {
    for name in [
        "shit",
        "SHIT",
        "ShIt",
        "sh1t",
        "5h1t",
        "s.h.i.t",
        "s h i t",
        "s_h_i_t",
        "s-h-i-t",
        "a$$hole",
        "b!tch",
        "phuck",
        "fuck_you_2024",
        "my_sh1t_texture",
        "IMG_0101_fuck",
        "kill yourself",
    ] {
        assert!(has_profanity(name), "{name} was allowed as a username");
        assert!(has_profanity_in_asset_name(name), "{name} was allowed as an asset name");
    }
}

// A false positive here blocks somebody from picking a perfectly ordinary name, which is worse than
// it sounds: there is no appeal, just a form that refuses them. Words that contain a shorter word
// are the usual way a filter gets this wrong.
#[test]
fn ordinary_names_are_allowed_including_words_that_contain_a_shorter_word() {
    for text in [
        "assassin",
        "classic",
        "analysis",
        "Scunthorpe",
        "Cockburn",
        "nice shot everyone",
        "Blockbuster",
        "hello 🌍",
        "",
    ] {
        assert!(!has_profanity(text), "{text} was refused as a username");
    }
}

// Names, asset names and chat block the same things: profane and sexual words from moderate up, offensive words at any
// level, and mean words only when severe. Everything milder is fine, so a game can be called Fat Cat or Bouncy Balls.
#[test]
fn milder_words_spam_and_shouting_are_allowed_everywhere() {
    for text in [
        "fat",
        "Fat Cat",
        "fat pig",
        "Poop Simulator",
        "stupid",
        "crap",
        "loser",
        "ugly",
        "Bouncy Balls",
        "Marry Me Simulator",
        "Snapchat Clone",
        "HELLO EVERYONE LOOK AT ME",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ] {
        assert!(!has_profanity(text), "{text} was refused as a username");
        assert!(!has_profanity_in_asset_name(text), "{text} was refused as an asset name");
        assert_eq!(censor(text), text, "{text} was censored in chat");
    }
}

#[test]
fn swearing_sexual_words_slurs_and_death_wishes_are_blocked_everywhere() {
    for text in ["shit", "nude", "porn", "Nazi Zombies", "Get Cancer"] {
        assert!(has_profanity(text), "{text} was allowed as a username");
        assert!(has_profanity_in_asset_name(text), "{text} was allowed as an asset name");
        assert_ne!(censor(text), text, "{text} was not censored in chat");
    }
}

// The two functions are configured separately, so nothing but a test keeps them agreeing. A word
// the chat room censors has to be a word the website refuses, or somebody can wear as a username
// the exact word the game will not let them say.
#[test]
fn anything_chat_censors_is_something_the_website_refuses() {
    for text in ["shit", "sh1t", "s h i t", "fuck this lag", "SHIT"] {
        assert_ne!(censor(text), text, "{text} is not censored, so this says nothing");
        assert!(has_profanity(text), "chat censors {text} but the website would allow it as a name");
    }
}
