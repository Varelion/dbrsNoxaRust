//! How a feat reads in a staff reply and in #mod-log:
//!
//! ```text
//!   ⚔️ Victory **F-0012** · counts Fight · Itami (@zibe) · [post 1](…) [post 2](…)
//!   > Beat Kewa; Kewa yielded.
//! ```

use crate::domain::feats::FeatKind;
use crate::domain::links::MessageLink;
use crate::services::feats::{FeatView, PostRef};

const NOTE_SHOWN: usize = 300;
const CUT: char = '…';

/// "logged …" ─► "Logged …": a log line, said back to the one who made it.
pub(crate) fn capitalized(line: &str) -> String {
    let mut chars = line.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// At most `max` characters, the last one an ellipsis when cut.
pub(crate) fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut kept: String = text.chars().take(max.saturating_sub(1)).collect();
    kept.push(CUT);
    kept
}

// Markdown would let a note restyle the log; these characters are shown as typed instead.
pub(crate) fn plain(text: &str) -> String {
    text.chars().fold(String::with_capacity(text.len()), |mut out, c| {
        if matches!(c, '*' | '_' | '~' | '`' | '|' | '>' | '#' | '[' | ']') {
            out.push('\\');
        }
        out.push(c);
        out
    })
}

pub(crate) fn post_link(guild_id: u64, post: PostRef, label: &str) -> String {
    let link = MessageLink { guild_id, channel_id: post.channel_id, message_id: post.message_id };
    format!("[{label}]({})", link.url())
}

pub(crate) fn posts_of(guild_id: u64, feat: &FeatView) -> String {
    let posts: Vec<_> = (1..)
        .zip(&feat.links)
        .map(|(n, l)| post_link(guild_id, PostRef { channel_id: l.channel_id, message_id: l.message_id }, &format!("post {n}")))
        .collect();
    if posts.is_empty() {
        return "no post".to_owned();
    }
    posts.join(" ")
}

pub(crate) fn feat_line(guild_id: u64, feat: &FeatView) -> String {
    format!(
        "{} **{}** · counts {} · {} (<@{}>) · {}",
        feat.kind.label(),
        feat.code,
        feat.counts.label(),
        plain(&feat.character_name),
        feat.player_id,
        posts_of(guild_id, feat)
    )
}

pub(crate) fn feat_block(guild_id: u64, feat: &FeatView) -> String {
    let note = plain(&cut(&feat.note, NOTE_SHOWN)).replace('\n', "\n> ");
    format!("{}\n> {note}", feat_line(guild_id, feat))
}

/// What an edit changed: "kind ⚔️ Victory → 🩸 Defeat · note rewritten".
pub(crate) fn changes_of(before: &FeatView, after: &FeatView) -> String {
    let mut parts = Vec::new();
    if before.kind != after.kind {
        parts.push(format!("kind {} → {}", before.kind.label(), after.kind.label()));
    }
    if before.counts != after.counts {
        parts.push(format!("counts {} → {}", before.counts.label(), after.counts.label()));
    }
    if before.note != after.note {
        parts.push("note rewritten".to_owned());
    }
    parts.join(" · ")
}

/// "F-0012 · Itami · ⚔️ Victory · Beat Kewa…", for a menu.
pub(crate) fn menu_line(code: &str, character: &str, kind: FeatKind, note: &str, max: usize) -> String {
    cut(&format!("{code} · {character} · {} · {note}", kind.label()), max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cut_keeps_short_text_and_ends_long_text_with_an_ellipsis() {
        assert_eq!(cut("Itami", 10), "Itami");
        assert_eq!(cut("Beat Kewa; Kewa yielded.", 10), "Beat Kewa…");
    }

    #[test]
    fn plain_shows_markdown_as_typed() {
        assert_eq!(plain("**bold** _x_"), "\\*\\*bold\\*\\* \\_x\\_");
    }
}
