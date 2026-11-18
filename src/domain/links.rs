//! A Discord message link, as "Copy Message Link" gives it:
//!
//! ```text
//!   https://discord.com/channels/585653958962577418/1302…/1311…  ─► MessageLink { guild, channel, message }
//! ```
//!
//! ptb. and canary. hosts, the old discordapp.com, and Discord's <…> wrapping are the same link.

const HOSTS: [&str; 4] = ["discord.com", "ptb.discord.com", "canary.discord.com", "discordapp.com"];
const SCHEMES: [&str; 2] = ["https://", "http://"];
const CHANNELS_PATH: &str = "/channels/";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MessageLink {
    pub(crate) guild_id: u64,
    pub(crate) channel_id: u64,
    pub(crate) message_id: u64,
}

impl MessageLink {
    pub(crate) fn url(&self) -> String {
        format!("https://discord.com/channels/{}/{}/{}", self.guild_id, self.channel_id, self.message_id)
    }
}

/// The link's parts; `None` when the text is not a message link.
pub(crate) fn parse_link(text: &str) -> Option<MessageLink> {
    let trimmed = text.trim().trim_start_matches('<').trim_end_matches('>');
    let rest = SCHEMES.iter().find_map(|scheme| trimmed.strip_prefix(scheme))?;
    let path = HOSTS.iter().find_map(|host| rest.strip_prefix(host)?.strip_prefix(CHANNELS_PATH))?;

    let mut ids = path.trim_end_matches('/').split('/').map(|part| part.parse::<u64>().ok());
    let (guild_id, channel_id, message_id) = (ids.next()??, ids.next()??, ids.next()??);
    if ids.next().is_some() {
        return None;
    }
    Some(MessageLink { guild_id, channel_id, message_id })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GUILD: u64 = 585_653_958_962_577_418;

    #[test]
    fn reads_copy_message_link_on_any_discord_host() {
        let post = MessageLink { guild_id: GUILD, channel_id: 700, message_id: 7001 };
        assert_eq!(parse_link(&format!("https://discord.com/channels/{GUILD}/700/7001")), Some(post));
        assert_eq!(parse_link(&format!("<https://ptb.discord.com/channels/{GUILD}/700/7001>")), Some(post));
        assert_eq!(parse_link(&format!("https://discordapp.com/channels/{GUILD}/700/7001/")), Some(post));
        assert_eq!(post.url(), format!("https://discord.com/channels/{GUILD}/700/7001"));
    }

    #[test]
    fn anything_else_is_none() {
        assert_eq!(parse_link(&format!("https://discord.com/channels/{GUILD}/700")), None);
        assert_eq!(parse_link(&format!("https://discord.com/channels/{GUILD}/700/7001/9")), None);
        assert_eq!(parse_link("#kassa-city"), None);
        assert_eq!(parse_link(&format!("https://evil.example/channels/{GUILD}/700/7001")), None);
        assert_eq!(parse_link(&format!("https://discord.com.evil.example/channels/{GUILD}/700/7001")), None);
    }
}
