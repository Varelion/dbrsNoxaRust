//! Easter eggs, Bot1's: a message that is exactly a trigger gets its answer; SHEESH anywhere gets one
//! more E. `.cookie` opens one of Curie's Curious Cookies.
//!
//! ```text
//!   "caw!"        ─► CAW!                       "how r u" ─► cow r moo, and back
//!   "kamehameha"  ─► KA… ME… HA… ME… HAAAA! 🌊   "sheeesh, that hurt" ─► SHEEEESH
//!   ".cookie"     ─► ㊁ Curie's Curious Cookies ㊁: a fortune, out of 168
//! ```

use twilight_model::channel::message::Embed;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFooterBuilder, ImageSource};

use crate::services::balls::random_below;

/// Exact triggers, matched lowercase and trimmed, with their answers.
const EGGS: &[(&[&str], &str)] = &[
    (&["caw!"], "CAW!"),
    (&["how r u"], "cow r moo"),
    (&["cow r moo"], "how r u"),
    (&["kakarot", "kakarot!", "kakarotto"], "**KAKAROTTOOOOOO!!!**"),
    (&["kamehameha", "kamehameha!"], "KA… ME… HA… ME… **HAAAAAAAAA!** 🌊"),
    (&["it's over 9000", "it's over 9000!", "its over 9000", "over 9000", "over 9000!"], "*crushes scouter* WHAT?! NINE THOUSAND?! **THERE'S NO WAY THAT CAN BE RIGHT!**"),
    (
        &["eternal dragon, by your name i summon you forth", "arise shenron", "arise, shenron"],
        "🐉 *The sky goes black.* **STATE YOUR WISH. I SHALL GRANT ANY ONE WISH.**\n-# Dragon Balls are found in play, not here.",
    ),
    (&["yamcha", "f for yamcha"], "*Yamcha lies face down in a crater, curled up, eyes closed.* 🫡"),
];

pub(crate) const COOKIE: &str = ".cookie";
const FORTUNES: &str = include_str!("../../data/fortunes.txt");
const COOKIE_TITLE: &str = "㊁ Curie's Curious Cookies ㊁";
const COOKIE_BANNER: &str = "CURIE'S CURIOUS COOKIES";
const COOKIE_COLOR: u32 = 0x9B_59_B6;
const COOKIE_THUMBNAIL: &str = "https://i.imgur.com/ZT0Nwr7.png";
const COOKIE_FOOTER: &str = "Curie's cookies are full of surprises. Use .cookie again for another!";
const BANNER_WIDTH: usize = 28;

// SHESH ─► SHEESH: one more E than written, as a whole word anywhere in the message.
fn sheesh(content: &str) -> Option<String> {
    let lower = content.to_lowercase();
    let bytes = lower.as_bytes();
    let word = |at: usize| bytes.get(at).is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_');
    let mut start = 0;
    while let Some(found) = lower[start..].find("sh") {
        let at = start + found;
        let es = lower[at + 2..].bytes().take_while(|b| *b == b'e').count();
        let end = at + 2 + es;
        let boundary_before = at == 0 || !word(at - 1);
        if es > 0 && boundary_before && lower[end..].starts_with("sh") && !word(end + 2) {
            return Some(format!("SH{}SH", "E".repeat(es + 1)));
        }
        start = at + 1;
    }
    None
}

/// The answer to a message, if it is an egg.
pub(crate) fn answer(content: &str) -> Option<String> {
    let typed = content.trim().to_lowercase();
    let typed = typed.strip_prefix('!').map_or(typed.clone(), |rest| format!(".{rest}"));
    if let Some((_, reply)) = EGGS.iter().find(|(triggers, _)| triggers.contains(&typed.as_str())) {
        return Some((*reply).to_owned());
    }
    sheesh(content)
}

pub(crate) fn is_cookie(content: &str) -> bool {
    content.trim().eq_ignore_ascii_case(COOKIE)
}

/// A fortune cookie, drawn at random.
pub(crate) fn cookie() -> anyhow::Result<Embed> {
    let fortunes: Vec<&str> = FORTUNES.lines().filter(|l| !l.trim().is_empty()).collect();
    let fortune = fortunes[random_below(fortunes.len())];
    let rule = "━".repeat(31);
    let edge = "═".repeat(34);
    let description = format!("```\n╔{edge}╗\n║    {COOKIE_BANNER:<BANNER_WIDTH$}    ║\n╚{edge}╝\n\n{fortune}\n\n{rule}\n```");
    Ok(EmbedBuilder::new()
        .title(COOKIE_TITLE)
        .color(COOKIE_COLOR)
        .description(description)
        .thumbnail(ImageSource::url(COOKIE_THUMBNAIL)?)
        .footer(EmbedFooterBuilder::new(COOKIE_FOOTER))
        .build())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheesh_gets_one_more_e_anywhere_as_a_word() {
        assert_eq!(answer("shesh"), Some("SHEESH".to_owned()));
        assert_eq!(answer("SHEEESH"), Some("SHEEEESH".to_owned()));
        assert_eq!(answer("ok sheesh, that hurt"), Some("SHEEESH".to_owned()));
        assert_eq!(answer("shsh"), None);
        assert_eq!(answer("bushesh"), None);
        assert_eq!(answer("sheeshing"), None);
    }

    #[test]
    fn eggs_match_the_whole_message_only() {
        assert_eq!(answer("  Caw! "), Some("CAW!".to_owned()));
        assert_eq!(answer("how r u"), Some("cow r moo".to_owned()));
        assert_eq!(answer("KAMEHAMEHA!"), Some("KA… ME… HA… ME… **HAAAAAAAAA!** 🌊".to_owned()));
        assert_eq!(answer("he fired a kamehameha"), None);
        assert_eq!(answer("hello"), None);
    }

    #[test]
    fn every_fortune_is_one_line_and_the_cookie_fits_an_embed() {
        assert_eq!(FORTUNES.lines().count(), 168);
        let embed = cookie().unwrap();
        assert!(twilight_validate::embed::embed(&embed).is_ok());
        assert!(is_cookie(".COOKIE") && !is_cookie(".cookies"));
    }
}
