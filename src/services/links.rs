//! One-time web links for staff: a token in a URL stands for one staff member, for a while. Kept in
//! memory: a restart ends every link, and staff ask for a new one.
//!
//! ```text
//!   /z_moderator_admin   ─► issue(staff, Staff)   ─► DM: https://…/a/<token>   128 random bits
//!   /me                  ─► issue(player, Player) ─► DM: https://…/p/<token>
//!   every page and form  ─► open(token, kind)     ─► their id, or None: expired (30 idle minutes,
//!                                                    4 hours), replaced, or the other kind of link
//!   a form's outcome     ─► tell(token)  ─► told(token) on the page it redirects to, once
//! ```
//!
//! A player's link never opens the admin pages: each page asks for its own kind.

use std::collections::HashMap;
use std::io::Read;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

const TOKEN_BYTES: usize = 16;
const IDLE: Duration = Duration::from_secs(30 * 60);
const LIFETIME: Duration = Duration::from_secs(4 * 60 * 60);

/// What a form did, shown once on the page after it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Notice {
    Done(String),
    Refused(String),
}

/// Which pages a link opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Access {
    Staff,
    Player,
}

struct Link {
    user_id: u64,
    access: Access,
    issued: Instant,
    used: Instant,
    notice: Option<Notice>,
}

#[derive(Default)]
pub(crate) struct Links {
    open: Mutex<HashMap<String, Link>>,
}

// The kernel's randomness: tokens must not be guessable.
fn token() -> Result<String> {
    let mut bytes = [0u8; TOKEN_BYTES];
    std::fs::File::open("/dev/urandom").and_then(|mut random| random.read_exact(&mut bytes)).context("reading /dev/urandom")?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

impl Links {
    /// A new link; the person's older link of the same kind stops working.
    pub(crate) fn issue(&self, user_id: u64, access: Access) -> Result<String> {
        let token = token()?;
        let now = Instant::now();
        let mut open = self.open.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        open.retain(|_, link| Self::alive(link, now) && !(link.user_id == user_id && link.access == access));
        open.insert(token.clone(), Link { user_id, access, issued: now, used: now, notice: None });
        Ok(token)
    }

    /// Who a live link of this kind stands for; using it keeps it alive.
    pub(crate) fn open(&self, token: &str, access: Access) -> Option<u64> {
        let now = Instant::now();
        let mut open = self.open.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let link = open.get_mut(token).filter(|link| link.access == access && Self::alive(link, now))?;
        link.used = now;
        Some(link.user_id)
    }

    /// Keeps a notice for the next page this link opens.
    pub(crate) fn tell(&self, token: &str, notice: Notice) {
        let mut open = self.open.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(link) = open.get_mut(token) {
            link.notice = Some(notice);
        }
    }

    /// The notice kept for this link, taken: it shows once.
    pub(crate) fn told(&self, token: &str) -> Option<Notice> {
        let mut open = self.open.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        open.get_mut(token)?.notice.take()
    }

    fn alive(link: &Link, now: Instant) -> bool {
        now.duration_since(link.used) < IDLE && now.duration_since(link.issued) < LIFETIME
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_opens_its_own_pages_for_its_person_and_nothing_else_does() {
        let links = Links::default();
        let token = links.issue(9, Access::Staff).unwrap();
        assert_eq!(token.len(), TOKEN_BYTES * 2);
        assert_eq!(links.open(&token, Access::Staff), Some(9));
        assert_eq!(links.open(&token, Access::Player), None);
        assert_eq!(links.open("0123456789abcdef0123456789abcdef", Access::Staff), None);

        let player = links.issue(157, Access::Player).unwrap();
        assert_eq!(links.open(&player, Access::Player), Some(157));
        assert_eq!(links.open(&player, Access::Staff), None);
    }

    #[test]
    fn a_new_link_replaces_the_old_one_of_its_kind_only() {
        let links = Links::default();
        let staff = links.issue(9, Access::Staff).unwrap();
        let first = links.issue(9, Access::Player).unwrap();
        let second = links.issue(9, Access::Player).unwrap();
        assert_ne!(first, second);
        assert_eq!(links.open(&first, Access::Player), None);
        assert_eq!(links.open(&second, Access::Player), Some(9));
        assert_eq!(links.open(&staff, Access::Staff), Some(9));
    }

    #[test]
    fn a_notice_shows_once() {
        let links = Links::default();
        let token = links.issue(9, Access::Staff).unwrap();
        links.tell(&token, Notice::Done("Saved.".to_owned()));
        assert_eq!(links.told(&token), Some(Notice::Done("Saved.".to_owned())));
        assert_eq!(links.told(&token), None);
    }
}
