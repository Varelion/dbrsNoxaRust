//! Upload tickets: what the bot signs so the relay Worker may post one image for one character, and
//! checks again when the Worker hands back the link. The Worker's side: neoKai relay-dbrs/uploads.js.
//!
//! ```text
//!   <payload>.<sig>                  both base64url, no padding
//!   payload  {"v":1,"g":guild,"u":uploader,"o":owner,"c":character,"k":"picture"|"expression",
//!             "x":expression|null,"cap":caption,"by":"staff"|"player","xc":colour,"exp":unix s,"n":nonce}
//!   sig      HMAC-SHA256(key: RELAY_SECRET, message: the payload's base64url text)
//! ```
//!
//! Ids travel as text: a snowflake is too big for a JavaScript number. "by" and "xc" are DBRS's own;
//! the Worker reads only "cap" and "exp".

use std::fmt::Display;
use std::str::FromStr;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as BASE64URL;
use ring::hmac;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub(crate) const VERSION: u8 = 1;
/// A ticket is good for five minutes: long enough to shrink and send a picture, too short to keep.
pub(crate) const TTL_S: u64 = 300;
const SEPARATOR: char = '.';

/// What an upload becomes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Kind {
    Picture,
    Expression,
}

/// Who asked for the ticket: staff on the admin pages, or the character's player on their page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum By {
    Staff,
    Player,
}

/// What a ticket allows: one image, for one character, captioned by the bot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Grant {
    #[serde(rename = "g", with = "as_text")]
    pub(crate) guild: u64,
    #[serde(rename = "u", with = "as_text")]
    pub(crate) uploader: u64,
    #[serde(rename = "o", with = "as_text")]
    pub(crate) owner: u64,
    #[serde(rename = "c", with = "as_text")]
    pub(crate) character: i64,
    #[serde(rename = "k")]
    pub(crate) kind: Kind,
    /// The expression's name; None for a picture.
    #[serde(rename = "x")]
    pub(crate) expression: Option<String>,
    /// The line the Worker posts with the image in #pictures.
    #[serde(rename = "cap")]
    pub(crate) caption: String,
    pub(crate) by: By,
    /// The expression's colour bar as typed, "" for none.
    #[serde(rename = "xc", default)]
    pub(crate) color: String,
}

/// A whole ticket: the grant, when it ends, and the nonce that makes it single use.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Ticket {
    pub(crate) v: u8,
    #[serde(flatten)]
    pub(crate) grant: Grant,
    pub(crate) exp: u64,
    #[serde(rename = "n")]
    pub(crate) nonce: String,
}

/// Why a ticket is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Broken {
    /// Not two base64url parts, or not a version-1 payload.
    Malformed,
    /// The signature does not match: not signed with this secret, or changed since.
    Forged,
    Expired,
}

// Ids as JSON strings: "1180000000000000001".
mod as_text {
    use super::{Deserialize, Deserializer, Display, FromStr, Serializer};

    pub(super) fn serialize<T: Display, S: Serializer>(value: &T, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(value)
    }

    pub(super) fn deserialize<'de, T: FromStr, D: Deserializer<'de>>(deserializer: D) -> Result<T, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(|_| serde::de::Error::custom("not an id"))
    }
}

fn key(secret: &[u8]) -> hmac::Key {
    hmac::Key::new(hmac::HMAC_SHA256, secret)
}

/// A payload's ticket: `<payload>.<sig>`. The payload is JSON text, signed as it stands.
pub(crate) fn sign(secret: &[u8], payload: &str) -> String {
    let payload = BASE64URL.encode(payload);
    let sig = hmac::sign(&key(secret), payload.as_bytes());
    format!("{payload}{SEPARATOR}{}", BASE64URL.encode(sig.as_ref()))
}

/// A ticket for `ticket`, signed.
pub(crate) fn seal(secret: &[u8], ticket: &Ticket) -> String {
    let payload = serde_json::to_string(ticket).unwrap_or_default();
    sign(secret, &payload)
}

/// The ticket inside `text`, once its signature and expiry are checked against `now_s`.
pub(crate) fn open(secret: &[u8], text: &str, now_s: u64) -> Result<Ticket, Broken> {
    let Some((payload, sig)) = text.split_once(SEPARATOR) else {
        return Err(Broken::Malformed);
    };
    let sig = BASE64URL.decode(sig).map_err(|_| Broken::Malformed)?;
    // Checked first, in constant time: nothing unsigned is even parsed.
    hmac::verify(&key(secret), payload.as_bytes(), &sig).map_err(|_| Broken::Forged)?;

    let json = BASE64URL.decode(payload).map_err(|_| Broken::Malformed)?;
    let ticket: Ticket = serde_json::from_slice(&json).map_err(|_| Broken::Malformed)?;
    if ticket.v != VERSION {
        return Err(Broken::Malformed);
    }
    if ticket.exp <= now_s {
        return Err(Broken::Expired);
    }
    Ok(ticket)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The shared test vector: the same three values are checked in neoKai relay-dbrs/uploads.test.js.
    const SECRET: &str = "dbrs-test-secret";
    const PAYLOAD: &str = r#"{"v":1,"g":"1","u":"157","o":"157","c":"42","k":"picture","x":null,"cap":"🖼️ **Captain Nova** · picture","by":"player","xc":"","exp":1800000000,"n":"0123456789abcdef"}"#;
    const TICKET: &str = "eyJ2IjoxLCJnIjoiMSIsInUiOiIxNTciLCJvIjoiMTU3IiwiYyI6IjQyIiwiayI6InBpY3R1cmUiLCJ4IjpudWxsLCJjYXAiOiLwn5a877iPICoqQ2FwdGFpbiBOb3ZhKiogwrcgcGljdHVyZSIsImJ5IjoicGxheWVyIiwieGMiOiIiLCJleHAiOjE4MDAwMDAwMDAsIm4iOiIwMTIzNDU2Nzg5YWJjZGVmIn0.fb0hnpL0MTOzR21Z1tdxA4h5_xn_tZTr7ibVx-nYtsE";
    const BEFORE: u64 = 1_700_000_000;
    const EXP: u64 = 1_800_000_000;

    fn nova() -> Ticket {
        let grant = Grant {
            guild: 1_180_000_000_000_000_001,
            uploader: 9,
            owner: 157,
            character: 42,
            kind: Kind::Expression,
            expression: Some("happy".to_owned()),
            caption: "🖼️ **Captain Nova** · `happy`".to_owned(),
            by: By::Staff,
            color: "#C0392B".to_owned(),
        };
        Ticket { v: VERSION, grant, exp: EXP, nonce: "00ff00ff00ff00ff".to_owned() }
    }

    #[test]
    fn the_shared_vector_signs_and_opens_alike_on_both_sides() {
        assert_eq!(sign(SECRET.as_bytes(), PAYLOAD), TICKET);
        let opened = open(SECRET.as_bytes(), TICKET, BEFORE).unwrap();
        assert_eq!((opened.grant.owner, opened.grant.character, opened.grant.kind), (157, 42, Kind::Picture));
        assert_eq!((opened.grant.caption.as_str(), opened.grant.by, opened.exp), ("🖼️ **Captain Nova** · picture", By::Player, EXP));
    }

    #[test]
    fn a_sealed_ticket_opens_whole_with_big_ids_kept_exact() {
        let sealed = seal(SECRET.as_bytes(), &nova());
        assert_eq!(open(SECRET.as_bytes(), &sealed, BEFORE), Ok(nova()));
        assert!(sealed.split('.').next().is_some_and(|p| !p.contains('=')));
    }

    #[test]
    fn a_changed_or_foreign_ticket_is_forged_and_an_old_one_expired() {
        let sealed = seal(SECRET.as_bytes(), &nova());
        assert_eq!(open(b"another secret", &sealed, BEFORE), Err(Broken::Forged));

        let mut other = nova();
        other.grant.character = 43;
        let (payload, _) = seal(SECRET.as_bytes(), &other).split_once('.').map(|(p, s)| (p.to_owned(), s.to_owned())).unwrap();
        let (_, sig) = sealed.split_once('.').unwrap();
        assert_eq!(open(SECRET.as_bytes(), &format!("{payload}.{sig}"), BEFORE), Err(Broken::Forged));

        assert_eq!(open(SECRET.as_bytes(), &sealed, EXP), Err(Broken::Expired));
        assert_eq!(open(SECRET.as_bytes(), "no dot here", BEFORE), Err(Broken::Malformed));
        assert_eq!(open(SECRET.as_bytes(), "a.b.c", BEFORE), Err(Broken::Malformed));
    }
}
