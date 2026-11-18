//! NPCs: the cast account's characters. Any staff member edits them; none is a player's.

/// The cast's "player": no Discord account has id 0.
pub(crate) const CAST: u64 = 0;

const PREFIX_MIN: usize = 2;
const PREFIX_MAX: usize = 12;
// Plain talk like "Note: …" must never post as someone.
const RESERVED: [&str; 5] = ["ooc", "note", "edit", "ps", "btw"];

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum BadPrefix {
    Shape,
    Reserved,
}

impl BadPrefix {
    pub(crate) const fn reason(&self) -> &'static str {
        match self {
            Self::Shape => "2 to 12 lowercase letters or digits",
            Self::Reserved => "reserved: ooc, note, edit, ps and btw never post as anyone",
        }
    }
}

/// "  Yen " ─► "yen"; empty ─► None (no prefix).
pub(crate) fn clean_prefix(text: &str) -> Result<Option<String>, BadPrefix> {
    let prefix = text.trim().to_lowercase();
    if prefix.is_empty() {
        return Ok(None);
    }
    let shaped = (PREFIX_MIN..=PREFIX_MAX).contains(&prefix.len()) && prefix.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    if !shaped {
        return Err(BadPrefix::Shape);
    }
    if RESERVED.contains(&prefix.as_str()) {
        return Err(BadPrefix::Reserved);
    }
    Ok(Some(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_are_short_lowercase_and_not_plain_words() {
        assert_eq!(clean_prefix("  Yen "), Ok(Some("yen".to_owned())));
        assert_eq!(clean_prefix(""), Ok(None));
        assert_eq!(clean_prefix("y"), Err(BadPrefix::Shape));
        assert_eq!(clean_prefix("king yenma"), Err(BadPrefix::Shape));
        assert_eq!(clean_prefix("note"), Err(BadPrefix::Reserved));
    }
}
