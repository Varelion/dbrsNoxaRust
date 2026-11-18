//! Easter eggs, Bot1's: a message that is exactly a trigger gets its answer; SHEESH anywhere gets one
//! more E. `.cookie` opens one of Curie's Curious Cookies.
//!
//! ```text
//!   "caw!"        ─► CAW!                       "how r u" ─► cow r moo, and back
//!   "kakarot"     ─► Z Broly himself, by webhook, his face attached
//!   "kamehameha"  ─► KA… ME… HA… ME… HAAAA! 🌊   "sheeesh, that hurt" ─► SHEEEESH
//!   ".cookie"     ─► ㊁ Curie's Curious Cookies ㊁: a fortune, out of 168
//! ```

use twilight_model::channel::message::Embed;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFooterBuilder, ImageSource};

use super::places::Persona;
use crate::services::balls::random_below;

/// Broly's face: his webhook's avatar, and the picture he answers with.
pub(crate) const BROLY_JPEG: &[u8] = include_bytes!("../../data/broly.jpg");
pub(crate) const BROLY_FILE: &str = "broly.jpg";
pub(crate) const BROLY: Persona = Persona { name: "Z Broly", jpeg: Some(BROLY_JPEG) };

/// Who answers an egg.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Voice {
    Bot,
    Broly,
}

/// An egg's answer, and who says it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Egg {
    pub(crate) voice: Voice,
    pub(crate) text: String,
}

/// Exact triggers, matched lowercase and trimmed, with who answers and how.
const EGGS: &[(&[&str], Voice, &str)] = &[
    (&["caw!"], Voice::Bot, "CAW!"),
    (&["how r u"], Voice::Bot, "cow r moo"),
    (&["cow r moo"], Voice::Bot, "how r u"),
    (&["kakarot", "kakarot!", "kakarotto"], Voice::Broly, "**KAKAROTTOOOOOO!!!**"),
    (&["kamehameha", "kamehameha!"], Voice::Bot, "KA… ME… HA… ME… **HAAAAAAAAA!** 🌊"),
    (&["it's over 9000", "it's over 9000!", "its over 9000", "over 9000", "over 9000!"], Voice::Bot, "WHAT?! NINE THOUSAND?! **THERE'S NO WAY THAT CAN BE RIGHT!**"),
    (
        &["eternal dragon, by your name i summon you forth", "arise shenron", "arise, shenron"],
        Voice::Bot,
        "🐉 *The sky goes black.* **STATE YOUR WISH. I SHALL GRANT ANY ONE WISH.**\n-# Dragon Balls are found in play, not here.",
    ),
    (&["yamcha", "f for yamcha"], Voice::Bot, "*Yamcha lies face down in a crater, curled up, eyes closed.* 🫡"),
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
pub(crate) fn answer(content: &str) -> Option<Egg> {
    let typed = content.trim().to_lowercase();
    let typed = typed.strip_prefix('!').map_or(typed.clone(), |rest| format!(".{rest}"));
    if let Some((_, voice, reply)) = EGGS.iter().find(|(triggers, _, _)| triggers.contains(&typed.as_str())) {
        return Some(Egg { voice: *voice, text: (*reply).to_owned() });
    }
    sheesh(content).map(|text| Egg { voice: Voice::Bot, text })
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

    fn said(content: &str) -> Option<String> {
        answer(content).map(|egg| egg.text)
    }

    #[test]
    fn kakarot_is_brolys_to_answer() {
        assert_eq!(answer("Kakarot!").map(|egg| egg.voice), Some(Voice::Broly));
        assert_eq!(answer("caw!").map(|egg| egg.voice), Some(Voice::Bot));
        assert!(BROLY_JPEG.starts_with(&[0xFF, 0xD8]));
    }

    #[test]
    fn sheesh_gets_one_more_e_anywhere_as_a_word() {
        assert_eq!(said("shesh"), Some("SHEESH".to_owned()));
        assert_eq!(said("SHEEESH"), Some("SHEEEESH".to_owned()));
        assert_eq!(said("ok sheesh, that hurt"), Some("SHEEESH".to_owned()));
        assert_eq!(said("shsh"), None);
        assert_eq!(said("bushesh"), None);
        assert_eq!(said("sheeshing"), None);
    }

    #[test]
    fn eggs_match_the_whole_message_only() {
        assert_eq!(said("  Caw! "), Some("CAW!".to_owned()));
        assert_eq!(said("how r u"), Some("cow r moo".to_owned()));
        assert_eq!(said("KAMEHAMEHA!"), Some("KA… ME… HA… ME… **HAAAAAAAAA!** 🌊".to_owned()));
        assert_eq!(said("he fired a kamehameha"), None);
        assert_eq!(said("hello"), None);
    }

    #[test]
    fn every_fortune_is_one_line_and_the_cookie_fits_an_embed() {
        assert_eq!(FORTUNES.lines().count(), 168);
        let embed = cookie().unwrap();
        assert!(twilight_validate::embed::embed(&embed).is_ok());
        assert!(is_cookie(".COOKIE") && !is_cookie(".cookies"));
    }
}
