//! Upload tickets, issued and spent: each one signed with RELAY_SECRET, good for five minutes, and
//! taken once. The nonces spent are kept in memory until their tickets would have expired anyway.
//!
//! ```text
//!   issue(grant)  ─► nonce (8 random bytes, hex) ─► "<payload>.<sig>"   to the page
//!   redeem(text)  ─► signature, expiry ─► nonce unspent? spend it ─► the grant
//! ```

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use anyhow::{Result, anyhow};
use ring::rand::{SecureRandom as _, SystemRandom};

use crate::domain::tickets::{Broken, Grant, TTL_S, Ticket, VERSION, open, seal};

const NONCE_BYTES: usize = 8;

/// Why a ticket is not taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refused {
    Broken(Broken),
    /// Its upload was already applied: a replayed frame.
    Spent,
}

pub(crate) struct Tickets {
    secret: Vec<u8>,
    random: SystemRandom,
    /// Spent nonces, and when their tickets expire.
    spent: Mutex<HashMap<String, u64>>,
}

impl Tickets {
    pub(crate) fn new(secret: &str) -> Self {
        Self { secret: secret.as_bytes().to_vec(), random: SystemRandom::new(), spent: Mutex::new(HashMap::new()) }
    }

    /// A ticket for `grant`, good until `now_s` + 5 minutes.
    pub(crate) fn issue(&self, grant: Grant, now_s: u64) -> Result<String> {
        let mut bytes = [0u8; NONCE_BYTES];
        self.random.fill(&mut bytes).map_err(|_| anyhow!("no randomness for an upload ticket's nonce"))?;
        let nonce = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let ticket = Ticket { v: VERSION, grant, exp: now_s + TTL_S, nonce };
        Ok(seal(&self.secret, &ticket))
    }

    /// The grant inside `text`, spent: the same ticket is refused from now on.
    pub(crate) fn redeem(&self, text: &str, now_s: u64) -> Result<Grant, Refused> {
        let ticket = open(&self.secret, text, now_s).map_err(Refused::Broken)?;
        let mut spent = self.spent.lock().unwrap_or_else(PoisonError::into_inner);
        // An expired ticket is refused by its expiry: its nonce need not be kept.
        spent.retain(|_, exp| *exp > now_s);
        if spent.contains_key(&ticket.nonce) {
            return Err(Refused::Spent);
        }
        spent.insert(ticket.nonce, ticket.exp);
        Ok(ticket.grant)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tickets::{By, Kind};

    const NOW: u64 = 1_700_000_000;

    fn grant() -> Grant {
        Grant {
            guild: 1,
            uploader: 157,
            owner: 157,
            character: 42,
            kind: Kind::Picture,
            expression: None,
            caption: "🖼️ **Captain Nova** · picture".to_owned(),
            by: By::Player,
            color: String::new(),
        }
    }

    #[test]
    fn a_ticket_is_taken_once_and_only_before_it_expires() {
        let tickets = Tickets::new("secret");
        let text = tickets.issue(grant(), NOW).unwrap();
        assert_eq!(tickets.redeem(&text, NOW + 1), Ok(grant()));
        assert_eq!(tickets.redeem(&text, NOW + 2), Err(Refused::Spent));

        let late = tickets.issue(grant(), NOW).unwrap();
        assert_eq!(tickets.redeem(&late, NOW + TTL_S), Err(Refused::Broken(Broken::Expired)));
    }

    #[test]
    fn every_ticket_has_its_own_nonce_and_another_bot_cannot_sign_one() {
        let tickets = Tickets::new("secret");
        let (first, second) = (tickets.issue(grant(), NOW).unwrap(), tickets.issue(grant(), NOW).unwrap());
        assert_ne!(first, second);
        assert_eq!(tickets.redeem(&second, NOW), Ok(grant()));
        assert_eq!(tickets.redeem(&first, NOW), Ok(grant()));

        let stranger = Tickets::new("not the secret").issue(grant(), NOW).unwrap();
        assert_eq!(tickets.redeem(&stranger, NOW), Err(Refused::Broken(Broken::Forged)));
    }
}
