//! The one profanity filter the whole product uses, for game chat on the server and for usernames
//! and game titles on the website.
//!
//! The website reaches these through WebAssembly rather than its own word list, so a word that is
//! caught in one place is caught in all of them. It reads through the tricks people use to get a
//! word past a plain list, so sh1t and f.u.c.k are caught along with the word itself.
//!
//! The two functions differ only in what the caller does about a match. Chat censors, because a
//! player who typed one bad word should still be heard. A username or a game title is rejected
//! instead, since nobody can be called `####`. Asset names are mostly file names, so they get a
//! looser check of their own.

use rustrict::{Censor, Type};

// Hashes rather than asterisks, so a censored word does not read as text somebody emphasised.
const CENSOR_REPLACEMENT: char = '#';

// What every check blocks: names refuse it and chat censors it. Profane and sexual words count from moderate up, so mild
// ones like poop, crap, balls and marry are fine. Offensive words count at any level, since slurs show up even at mild. Mean
// words count only when severe, like death wishes. Spam, shouting, and the spellings people use to dodge the filter never
// count on their own.
fn blocked_types() -> Type {
    ((Type::PROFANE | Type::SEXUAL) & Type::MODERATE_OR_HIGHER) | Type::OFFENSIVE | (Type::MEAN & Type::SEVERE)
}

// Every matched character becomes one hash, so the text keeps its length and a caller that
// already checked a length limit does not have to check it again. The first character is censored
// along with the rest; leaving it would give away the word.
pub fn censor(text: &str) -> String {
    Censor::from_str(text)
        .with_censor_threshold(blocked_types())
        .with_censor_first_character_threshold(Type::ANY)
        .with_censor_replacement(CENSOR_REPLACEMENT)
        .censor()
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen(js_name = hasProfanity))]
pub fn has_profanity(text: &str) -> bool {
    Censor::from_str(text).analyze().is(blocked_types())
}

// Reading the whole name at once turns dates and version numbers into letters across the gaps between
// words: the 0101_1 in IMG_20240101_123456 reads as a sexual word. So each word is read as written, which
// still catches sh1t, and the name is read again without its digits, which still catches s.h.i.t.
// Capitals and gibberish are not bad words, so TEXTURE_ATLAS is fine.
pub fn has_profanity_in_asset_name(text: &str) -> bool {
    let is_bad = |text: &str| Censor::from_str(text).analyze().is(blocked_types());
    let text_without_digits: String = text.chars().filter(|character| !character.is_ascii_digit()).collect();
    is_bad(&text_without_digits)
        || text
            .split(|character: char| character.is_whitespace() || ['_', '-', '.'].contains(&character))
            .any(is_bad)
}
