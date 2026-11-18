//! Character pictures: which uploads are images, and when a Discord attachment link runs out.
//!
//! ```text
//!   upload bytes ─► kind_of ─► Png | Jpeg | Gif | Webp, or refused
//!   https://cdn.discordapp.com/attachments/…/nova.png?ex=6790a1b2&is=…&hm=…
//!                                                      └─ expires_at: 0x6790a1b2 seconds since the epoch
//! ```

/// Discord takes 10 MiB from a bot; a little under, so the upload form never meets Discord's refusal.
pub(crate) const UPLOAD_MAX: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImageKind {
    Png,
    Jpeg,
    Gif,
    Webp,
}

impl ImageKind {
    pub(crate) const fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Gif => "gif",
            Self::Webp => "webp",
        }
    }
}

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
const JPEG: &[u8] = b"\xff\xd8\xff";
const GIF: &[u8] = b"GIF8";
const RIFF: &[u8] = b"RIFF";
const WEBP: &[u8] = b"WEBP";
// "RIFF", four size bytes, then "WEBP".
const WEBP_AT: usize = 8;

/// What the bytes are, read from their first few: a file's name or claimed type is not trusted.
pub(crate) fn kind_of(bytes: &[u8]) -> Option<ImageKind> {
    if bytes.starts_with(PNG) {
        return Some(ImageKind::Png);
    }
    if bytes.starts_with(JPEG) {
        return Some(ImageKind::Jpeg);
    }
    if bytes.starts_with(GIF) {
        return Some(ImageKind::Gif);
    }
    if bytes.starts_with(RIFF) && bytes.get(WEBP_AT..WEBP_AT + WEBP.len()) == Some(WEBP) {
        return Some(ImageKind::Webp);
    }
    None
}

/// When a signed attachment link stops working, in seconds since the epoch; None for a link that never does.
pub(crate) fn expires_at(url: &str) -> Option<u64> {
    const HEX: u32 = 16;
    let (_, query) = url.split_once('?')?;
    let ex = query.split('&').find_map(|pair| pair.strip_prefix("ex="))?;
    u64::from_str_radix(ex, HEX).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_are_known_by_their_bytes() {
        assert_eq!(kind_of(b"\x89PNG\r\n\x1a\n...."), Some(ImageKind::Png));
        assert_eq!(kind_of(b"\xff\xd8\xff\xe0...."), Some(ImageKind::Jpeg));
        assert_eq!(kind_of(b"GIF89a.."), Some(ImageKind::Gif));
        assert_eq!(kind_of(b"RIFF\x10\x00\x00\x00WEBPVP8 "), Some(ImageKind::Webp));
        assert_eq!(kind_of(b"RIFF\x10\x00\x00\x00WAVEfmt "), None);
        assert_eq!(kind_of(b"<svg onload=alert(1)>"), None);
        assert_eq!(kind_of(b""), None);
    }

    #[test]
    fn signed_links_say_when_they_expire() {
        let url = "https://cdn.discordapp.com/attachments/1/2/nova.png?ex=6790a1b2&is=678f5032&hm=ab12";
        assert_eq!(expires_at(url), Some(0x6790_a1b2));
        assert_eq!(expires_at("https://i.imgur.com/nova.png"), None);
        assert_eq!(expires_at("https://cdn.discordapp.com/x.png?ex=zz"), None);
    }
}
