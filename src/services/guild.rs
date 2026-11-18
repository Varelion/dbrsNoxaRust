//! The port to Discord for the web pages: what they need from the server, answered by the Discord
//! layer. The web never calls Discord itself.
//!
//! ```text
//!   web ─► Guild::people / channels / upload / fresh / audit ─► Ask ─► discord::guild::answer ─► Discord
//!                    ◄──────────────────── oneshot reply ◄────────────────┘
//! ```
//!
//! When the Discord side is gone (shutting down, or a test without one), every ask answers empty.

use std::collections::HashMap;

use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::sync::oneshot;
use tracing::warn;

use crate::domain::pictures::ImageKind;
use crate::domain::tutorial::Did;
use crate::store::characters::CharacterRow;

/// Someone as the server shows them: their name there, and their picture there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Person {
    pub(crate) name: String,
    pub(crate) icon: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChannelKind {
    Category,
    Text,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Channel {
    pub(crate) id: u64,
    pub(crate) name: String,
    pub(crate) kind: ChannelKind,
    pub(crate) parent: Option<u64>,
    pub(crate) position: i32,
}

/// An image for the pictures channel.
pub(crate) struct Upload {
    pub(crate) bytes: Vec<u8>,
    pub(crate) kind: ImageKind,
    /// The line posted with it: whose picture, who uploaded it.
    pub(crate) caption: String,
    /// The file's name, without its extension: "captain-nova".
    pub(crate) stem: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Uploaded {
    pub(crate) channel_id: u64,
    pub(crate) message_id: u64,
    pub(crate) url: String,
}

impl Uploaded {
    /// The upload's message, for a mod log line: staff open it to see the picture.
    pub(crate) fn jump(&self, server: u64) -> String {
        format!("https://discord.com/channels/{server}/{}/{}", self.channel_id, self.message_id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UploadRefused {
    /// PICTURES_CHANNEL is not set.
    NoChannel,
    /// Discord said no: no access to the channel, or the file was refused.
    Refused,
    /// No Discord side to ask.
    Gone,
}

pub(crate) enum Ask {
    Audit(String),
    People { users: Vec<u64>, reply: oneshot::Sender<HashMap<u64, Person>> },
    Channels { reply: oneshot::Sender<Vec<Channel>> },
    Upload { upload: Upload, reply: oneshot::Sender<Result<Uploaded, UploadRefused>> },
    Fresh { links: Vec<String>, reply: oneshot::Sender<HashMap<String, String>> },
    /// Something a person did on the web, for their tutorial.
    Did { user: u64, did: Did },
}

#[derive(Clone)]
pub(crate) struct Guild {
    server: u64,
    asks: UnboundedSender<Ask>,
}

impl Guild {
    pub(crate) fn new(server: u64) -> (Self, UnboundedReceiver<Ask>) {
        let (asks, answers) = mpsc::unbounded_channel();
        (Self { server, asks }, answers)
    }

    /// The server's id, for links to its posts.
    pub(crate) const fn server(&self) -> u64 {
        self.server
    }

    /// A line for the mod log. Every change made on the web sends one.
    pub(crate) fn audit(&self, line: String) {
        if self.asks.send(Ask::Audit(line)).is_err() {
            warn!(target: "audit", "the mod log writer is gone; the line stays in this journal only");
        }
    }

    /// Names and pictures, as the server shows them; an id Discord does not know is left out.
    pub(crate) async fn people(&self, users: Vec<u64>) -> HashMap<u64, Person> {
        self.ask(|reply| Ask::People { users, reply }).await.unwrap_or_default()
    }

    /// Tells the tutorial what someone did on the web; its next step is posted in their thread.
    pub(crate) fn did(&self, user: u64, did: Did) {
        self.asks.send(Ask::Did { user, did }).ok();
    }

    /// Display names only.
    pub(crate) async fn names(&self, users: Vec<u64>) -> HashMap<u64, String> {
        self.people(users).await.into_iter().map(|(id, person)| (id, person.name)).collect()
    }

    pub(crate) async fn channels(&self) -> Vec<Channel> {
        self.ask(|reply| Ask::Channels { reply }).await.unwrap_or_default()
    }

    pub(crate) async fn upload(&self, upload: Upload) -> Result<Uploaded, UploadRefused> {
        self.ask(|reply| Ask::Upload { upload, reply }).await.unwrap_or(Err(UploadRefused::Gone))
    }

    /// A working link for each Discord link that expires, keyed by the stored one. A link that never
    /// expires, or cannot be signed again, is left out: use it as it is.
    pub(crate) async fn fresh(&self, links: Vec<String>) -> HashMap<String, String> {
        if links.is_empty() {
            return HashMap::new();
        }
        self.ask(|reply| Ask::Fresh { links, reply }).await.unwrap_or_default()
    }

    async fn ask<T>(&self, make: impl FnOnce(oneshot::Sender<T>) -> Ask) -> Option<T> {
        let (reply, answer) = oneshot::channel();
        self.asks.send(make(reply)).ok()?;
        answer.await.ok()
    }
}

/// The link to show: its fresh one, if it was signed again, else itself.
pub(crate) fn fresh_or(url: &str, fresh: &HashMap<String, String>) -> String {
    fresh.get(url).cloned().unwrap_or_else(|| url.to_owned())
}

/// The link to show for a character's picture, or None: their owner's icon.
pub(crate) fn picture_link(row: &CharacterRow, fresh: &HashMap<String, String>) -> Option<String> {
    row.picture_url.as_deref().map(|url| fresh_or(url, fresh))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn with_no_discord_side_every_ask_answers_empty() {
        let (guild, answers) = Guild::new(1);
        drop(answers);
        assert!(guild.names(vec![1]).await.is_empty());
        assert!(guild.channels().await.is_empty());
        let upload = Upload { bytes: Vec::new(), kind: ImageKind::Png, caption: String::new(), stem: String::new() };
        assert_eq!(guild.upload(upload).await, Err(UploadRefused::Gone));
        guild.audit("kept in the journal".to_owned());
    }
}
