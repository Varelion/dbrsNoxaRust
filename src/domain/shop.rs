//! The shop: emoji items players buy with Spirit Points, kept in their inventory.
//!
//! ```text
//!   🫘 Senzu Bean    25 SP   buy 2 ─► 50 SP spent ─► inventory: 🫘 Senzu Bean ×2
//!   🎁 Starter Capsule  free  one each
//! ```

use super::spirit::SP_MAX;

pub(crate) const NAME_MAX: usize = 40;
pub(crate) const DESCRIPTION_MAX: usize = 200;
/// Characters: a flag or a skin-toned emoji is several, a custom one `<:name:id>` more.
pub(crate) const EMOJI_MAX: usize = 64;
pub(crate) const PRICE_MAX: i64 = SP_MAX;
/// The most one purchase buys.
pub(crate) const BUY_MAX: i64 = 99;
/// The most of one item a player may own.
pub(crate) const OWN_MAX: i64 = 999;
/// The ledger kind of a purchase.
pub(crate) const BUY: &str = "buy";

/// How many of an item a player may own: a free one once, any other up to OWN_MAX.
pub(crate) const fn most_owned(price: i64) -> i64 {
    if price == 0 { 1 } else { OWN_MAX }
}

/// What `count` cost at `price`; None past what a balance holds.
pub(crate) const fn cost(price: i64, count: i64) -> Option<i64> {
    price.checked_mul(count)
}

/// "Free", "1 SP", "25 SP".
pub(crate) fn price_label(price: i64) -> String {
    if price == 0 { "Free".to_owned() } else { format!("{price} SP") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_free_item_is_owned_once() {
        assert_eq!(most_owned(0), 1);
        assert_eq!(most_owned(25), OWN_MAX);
        assert_eq!(cost(25, 2), Some(50));
        assert_eq!(cost(i64::MAX, 2), None);
        assert_eq!(price_label(0), "Free");
        assert_eq!(price_label(25), "25 SP");
    }
}
