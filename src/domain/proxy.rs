//! Posting as a character, Bot1's way: the rules, no I/O.
//!
//! ```text
//!   "nova: Hello"          ─► prefix nova, no expression, "Hello"
//!   "nova:happy Hello"     ─► prefix nova, expression happy   ("nova:happy: Hello" too)
//!   "nova:Hello"           ─► not a post as anyone: a space must follow the colon part
//!   a post over 1800       ─► parts, cut at a paragraph, else a line, a sentence, a word
//! ```

/// A prefix or an expression name: 1 to 20 of A-Z a-z 0-9 _ -, kept lowercase.
const NAME_MAX: usize = 20;
/// One part of a long post; room is left for the continued and reply lines.
pub(crate) const PART_MAX: usize = 1800;
// Plain talk like "Note: …" must never post as someone.
const RESERVED: [&str; 5] = ["ooc", "note", "edit", "ps", "btw"];
const FENCE: &str = "```";

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum BadPrefix {
    Shape,
    Reserved,
}

impl BadPrefix {
    pub(crate) const fn reason(&self) -> &'static str {
        match self {
            Self::Shape => "1 to 20 letters, digits, - or _",
            Self::Reserved => "reserved: ooc, note, edit, ps and btw never post as anyone",
        }
    }
}

fn shaped(name: &str) -> bool {
    (1..=NAME_MAX).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// "  Yen: " ─► "yen"; empty or "none" ─► None (no prefix).
pub(crate) fn clean_prefix(text: &str) -> Result<Option<String>, BadPrefix> {
    let prefix = text.trim().trim_end_matches(':').to_lowercase();
    if prefix.is_empty() || matches!(prefix.as_str(), "none" | "no" | "null") {
        return Ok(None);
    }
    if !shaped(&prefix) {
        return Err(BadPrefix::Shape);
    }
    if RESERVED.contains(&prefix.as_str()) {
        return Err(BadPrefix::Reserved);
    }
    Ok(Some(prefix))
}

/// An expression's name, as stored: "Happy:" ─► "happy"; None when it is not one.
pub(crate) fn clean_expression(text: &str) -> Option<String> {
    let name = text.trim().trim_end_matches(':').to_lowercase();
    shaped(&name).then_some(name)
}

/// A post as a character, read from a message.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Proxy<'a> {
    pub(crate) prefix: String,
    pub(crate) expression: Option<String>,
    pub(crate) body: &'a str,
}

/// "nova:happy Hello" ─► Proxy; any other message ─► None.
pub(crate) fn parse(content: &str) -> Option<Proxy<'_>> {
    let (head, rest) = content.split_once(':')?;
    if !shaped(head) {
        return None;
    }
    // After the colon: an expression glued to it, then whitespace, then the text.
    let cut = rest.find(char::is_whitespace)?;
    let (expression, body) = (&rest[..cut], rest[cut..].trim_start());
    if body.is_empty() {
        return None;
    }
    let expression = expression.trim_end_matches(':');
    let expression = match expression {
        "" => None,
        name if shaped(name) => Some(name.to_lowercase()),
        _ => return None,
    };
    Some(Proxy { prefix: head.to_lowercase(), expression, body })
}

// Where a long text is best cut, at most `max` bytes in: a paragraph, a line, a sentence, a word.
fn cut_at(text: &str, max: usize) -> usize {
    let mut limit = max.min(text.len());
    while !text.is_char_boundary(limit) {
        limit -= 1;
    }
    let window = &text[..limit];
    for mark in ["\n\n", "\n", ". ", "! ", "? ", ", ", " "] {
        if let Some(at) = window.rfind(mark).filter(|at| *at > limit / 3) {
            return at + mark.len();
        }
    }
    limit
}

/// A post in parts no longer than `max`. A code block cut in two is closed and opened again.
pub(crate) fn parts(text: &str, max: usize) -> Vec<String> {
    let fence_room = 2 * (FENCE.len() + 1);
    let mut out = Vec::new();
    let mut rest = text.trim();
    let mut reopen = false;
    while !rest.is_empty() {
        let room = if rest.len() + usize::from(reopen) * (FENCE.len() + 1) <= max { rest.len() } else { max - fence_room };
        let at = if room >= rest.len() { rest.len() } else { cut_at(rest, room) };
        let mut part = String::new();
        if reopen {
            part.push_str(FENCE);
            part.push('\n');
        }
        part.push_str(rest[..at].trim_end());
        let open = part.matches(FENCE).count() % 2 == 1;
        if open {
            part.push('\n');
            part.push_str(FENCE);
        }
        reopen = open;
        out.push(part);
        rest = rest[at..].trim_start();
    }
    out
}

/// The line a later part starts with, linking the part before it.
pub(crate) fn continued_line(part: usize, of: usize, previous_url: &str) -> String {
    format!("-# ↪ continued · [part {part}/{of}]({previous_url})")
}

pub(crate) const CONTINUED_MARK: &str = "-# ↪ continued";
pub(crate) const REPLY_MARK: &str = "> [Reply to](";

/// The line a reply ends with: who it answers, pinged.
pub(crate) fn reply_line(url: &str, name: &str, user: u64) -> String {
    format!("{REPLY_MARK}{url}): **{name}** (<@{user}>)")
}

/// A posted part split into its header lines and the text its author wrote: (continued, body, reply).
pub(crate) fn split_headers(content: &str) -> (Option<&str>, &str, Option<&str>) {
    let mut body = content;
    let mut continued = None;
    if body.starts_with(CONTINUED_MARK) {
        let (line, rest) = body.split_once('\n').unwrap_or((body, ""));
        continued = Some(line);
        body = rest;
    }
    let mut reply = None;
    if let Some(at) = body.rfind(&format!("\n{REPLY_MARK}")) {
        reply = Some(&body[at + 1..]);
        body = &body[..at];
    } else if body.starts_with(REPLY_MARK) {
        reply = Some(body);
        body = "";
    }
    (continued, body, reply)
}

/// A hex colour: "#C0392B", "c0392b" ─► 0xC0392B; blank ─► Ok(None).
pub(crate) fn parse_color(text: &str) -> Result<Option<u32>, ()> {
    const HEX: u32 = 16;
    const LEN: usize = 6;
    let hex = text.trim().trim_start_matches('#');
    if hex.is_empty() {
        return Ok(None);
    }
    if hex.len() != LEN {
        return Err(());
    }
    u32::from_str_radix(hex, HEX).map(Some).map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_are_short_words_and_not_plain_talk() {
        assert_eq!(clean_prefix("  Yen: "), Ok(Some("yen".to_owned())));
        assert_eq!(clean_prefix("c"), Ok(Some("c".to_owned())));
        assert_eq!(clean_prefix("k-2_x"), Ok(Some("k-2_x".to_owned())));
        assert_eq!(clean_prefix("none"), Ok(None));
        assert_eq!(clean_prefix(""), Ok(None));
        assert_eq!(clean_prefix("king yenma"), Err(BadPrefix::Shape));
        assert_eq!(clean_prefix(&"a".repeat(21)), Err(BadPrefix::Shape));
        assert_eq!(clean_prefix("Note"), Err(BadPrefix::Reserved));
    }

    #[test]
    fn a_post_as_a_character_is_prefix_colon_maybe_an_expression_then_text() {
        assert_eq!(parse("nova: Hello there"), Some(Proxy { prefix: "nova".to_owned(), expression: None, body: "Hello there" }));
        assert_eq!(parse("Nova:Happy Hi"), Some(Proxy { prefix: "nova".to_owned(), expression: Some("happy".to_owned()), body: "Hi" }));
        assert_eq!(parse("nova:happy: Hi\nsecond line"), Some(Proxy { prefix: "nova".to_owned(), expression: Some("happy".to_owned()), body: "Hi\nsecond line" }));
        assert_eq!(parse("nova:Hello"), None);
        assert_eq!(parse("nova:   "), None);
        assert_eq!(parse("two words: hello"), None);
        assert_eq!(parse("https://example.com hi"), None);
        assert_eq!(parse("just talking"), None);
    }

    #[test]
    fn long_posts_are_cut_at_paragraphs_then_lines_then_words() {
        let paragraph = "word ".repeat(200);
        let text = format!("{}\n\n{}", paragraph.trim(), paragraph.trim());
        let cut = parts(&text, 1200);
        assert_eq!(cut.len(), 2);
        assert!(cut.iter().all(|p| p.len() <= 1200));
        assert_eq!(parts("short", PART_MAX), ["short"]);

        let long = "x ".repeat(2000);
        assert!(parts(&long, PART_MAX).iter().all(|p| p.len() <= PART_MAX && !p.ends_with(' ')));
    }

    #[test]
    fn a_code_block_cut_in_two_is_closed_and_reopened() {
        let text = format!("```\n{}\n```", "line\n".repeat(600));
        let cut = parts(&text, PART_MAX);
        assert!(cut.len() > 1);
        assert!(cut.iter().all(|p| p.matches(FENCE).count() % 2 == 0 && p.len() <= PART_MAX));
    }

    #[test]
    fn headers_come_off_for_an_edit_and_go_back_on() {
        let content = format!("{}\nthe text\nmore\n{}", continued_line(2, 3, "u"), reply_line("v", "Goku", 1));
        let (continued, body, reply) = split_headers(&content);
        assert_eq!(continued, Some("-# ↪ continued · [part 2/3](u)"));
        assert_eq!(body, "the text\nmore");
        assert_eq!(reply, Some("> [Reply to](v): **Goku** (<@1>)"));
        assert_eq!(split_headers("plain"), (None, "plain", None));
    }

    #[test]
    fn colours_are_six_hex_digits() {
        assert_eq!(parse_color("#C0392B"), Ok(Some(0xC0_39_2B)));
        assert_eq!(parse_color(""), Ok(None));
        assert_eq!(parse_color("red"), Err(()));
    }
}
