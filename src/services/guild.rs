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
    pub(crate) message_id: u64,
    pub(crate) url: String,
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

/// An uploaded picture, as stored: its message, and the link it had last.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Stored {
    pub(crate) message_id: u64,
    pub(crate) url: String,
}

impl Stored {
    pub(crate) fn of(row: &CharacterRow) -> Option<Self> {
        Some(Self { message_id: row.picture_message?, url: row.picture_url.clone()? })
    }
}

pub(crate) enum Ask {
    Audit(String),
    People { users: Vec<u64>, reply: oneshot::Sender<HashMap<u64, Person>> },
    Channels { reply: oneshot::Sender<Vec<Channel>> },
    Upload { upload: Upload, reply: oneshot::Sender<Result<Uploaded, UploadRefused>> },
    Fresh { pictures: Vec<Stored>, reply: oneshot::Sender<HashMap<u64, String>> },
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

    /// A working link for each uploaded picture, by message id; one that cannot be refreshed is left out.
    pub(crate) async fn fresh(&self, pictures: Vec<Stored>) -> HashMap<u64, String> {
        if pictures.is_empty() {
            return HashMap::new();
        }
        self.ask(|reply| Ask::Fresh { pictures, reply }).await.unwrap_or_default()
    }

    async fn ask<T>(&self, make: impl FnOnce(oneshot::Sender<T>) -> Ask) -> Option<T> {
        let (reply, answer) = oneshot::channel();
        self.asks.send(make(reply)).ok()?;
        answer.await.ok()
    }
}

/// The link to show for a character's picture: a fresh one for an upload, else its own link.
pub(crate) fn picture_link(row: &CharacterRow, fresh: &HashMap<u64, String>) -> Option<String> {
    match row.picture_message {
        Some(message) => fresh.get(&message).cloned().or_else(|| row.picture_url.clone()),
        None => row.picture_url.clone(),
    }
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
