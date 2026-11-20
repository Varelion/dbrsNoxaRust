//! Character pictures: when a Discord attachment link runs out. Uploaded bytes are checked by the
//! relay Worker (neoKai relay-dbrs/uploads.js), never seen here.
//!
//! ```text
//!   https://cdn.discordapp.com/attachments/…/nova.png?ex=6790a1b2&is=…&hm=…
//!                                                      └─ expires_at: 0x6790a1b2 seconds since the epoch
//! ```

// Discord's own hosts: only their links are signed, and only they can be signed again.
const DISCORD_HOSTS: [&str; 2] = ["https://cdn.discordapp.com/", "https://media.discordapp.net/"];

/// When a signed attachment link stops working, in seconds since the epoch; None for a link that never does.
pub(crate) fn expires_at(url: &str) -> Option<u64> {
    const HEX: u32 = 16;
    if !DISCORD_HOSTS.iter().any(|host| url.starts_with(host)) {
        return None;
    }
    let (_, query) = url.split_once('?')?;
    let ex = query.split('&').find_map(|pair| pair.strip_prefix("ex="))?;
    u64::from_str_radix(ex, HEX).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_links_say_when_they_expire() {
        let url = "https://cdn.discordapp.com/attachments/1/2/nova.png?ex=6790a1b2&is=678f5032&hm=ab12";
        assert_eq!(expires_at(url), Some(0x6790_a1b2));
        assert_eq!(expires_at("https://i.imgur.com/nova.png"), None);
        assert_eq!(expires_at("https://cdn.discordapp.com/x.png?ex=zz"), None);
        assert_eq!(expires_at("https://example.com/x.png?ex=6790a1b2"), None);
    }
}
