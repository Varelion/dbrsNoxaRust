//! Players, for the admin pages: everyone with a wallet or a character, with their Spirit Points.

use anyhow::Result;

use crate::store::Store;
use crate::store::players::{self as repo, PlayerRow};

pub(crate) struct Players<'a> {
    store: &'a Store,
}

impl<'a> Players<'a> {
    pub(crate) const fn new(store: &'a Store) -> Self {
        Self { store }
    }

    pub(crate) fn all(&self) -> Result<Vec<PlayerRow>> {
        self.store.read(repo::all)
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::spirit::SpAction;
    use crate::services::Services;
    use crate::services::sheets::{SheetForm, Target};
    use crate::services::spirit::Change;

    #[test]
    fn players_are_wallets_and_character_owners_never_the_cast() {
        let services = Services::memory(0);
        services.characters().add(157, 1, "Captain Nova", 9).unwrap();
        services.characters().add(157, 2, "Nicolas", 9).unwrap();
        let change = Change { action: SpAction::Give, player_id: 410, amount: 5, memo: "event", staff_id: 9 };
        services.spirit().change(&change).unwrap();
        services.sheets().save(Target::NewNpc, &SheetForm { name: "King Yenma".to_owned(), ..SheetForm::default() }, 9).unwrap();

        let players = services.players().all().unwrap();
        let seen: Vec<_> = players.iter().map(|p| (p.player_id, p.sp, p.characters)).collect();
        assert_eq!(seen, [(157, 0, 2), (410, 5, 0)]);
    }
}
