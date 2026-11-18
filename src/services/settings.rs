//! noxaDBRS's settings. Set with /z_moderator_dbrs.
//!
//!   mod log: the channel every staff and bot change is posted to; none until set

use anyhow::Result;

use crate::store::{Store, settings};

const MOD_LOG: &str = "mod_log";

pub(crate) struct Settings<'a> {
    store: &'a Store,
}

impl<'a> Settings<'a> {
    pub(crate) const fn new(store: &'a Store) -> Self {
        Self { store }
    }

    pub(crate) fn mod_log(&self) -> Result<Option<u64>> {
        let value = self.store.read(|c| settings::get(c, MOD_LOG))?;
        Ok(value.and_then(|v| v.parse().ok()))
    }

    pub(crate) fn set_mod_log(&self, channel_id: u64) -> Result<()> {
        self.store.write(|tx| settings::set(tx, MOD_LOG, &channel_id.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use crate::services::Services;

    #[test]
    fn the_mod_log_is_unset_until_staff_set_it() {
        let services = Services::memory(0);
        assert_eq!(services.settings().mod_log().unwrap(), None);
        services.settings().set_mod_log(60).unwrap();
        services.settings().set_mod_log(61).unwrap();
        assert_eq!(services.settings().mod_log().unwrap(), Some(61));
    }
}
