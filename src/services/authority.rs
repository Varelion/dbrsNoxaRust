//! Who may change Spirit Points, feats and settings: administrators (the server owner counts: Discord
//! gives the owner every permission), anyone with Manage Server, and the server's staff roles.
//! Storytellers run NPCs, not wallets, so they are left out.

pub(crate) const STAFF_ROLES: [&str; 5] = ["Mega Ultra KEEPER", "Co-Owner", "Fate Keeper", "Admin", "Moderator"];

/// How the caller stands, as the Discord layer read it.
pub(crate) struct Caller<'a> {
    pub(crate) administrator: bool,
    pub(crate) manages_server: bool,
    pub(crate) role_names: &'a [String],
}

pub(crate) fn can_moderate(caller: &Caller) -> bool {
    if caller.administrator || caller.manages_server {
        return true;
    }
    caller.role_names.iter().any(|name| STAFF_ROLES.contains(&name.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn caller(role_names: &[String]) -> Caller<'_> {
        Caller { administrator: false, manages_server: false, role_names }
    }

    #[test]
    fn staff_roles_and_server_powers_moderate_others_do_not() {
        assert!(can_moderate(&caller(&["Moderator".to_owned()])));
        assert!(!can_moderate(&caller(&["Storyteller".to_owned(), "Saiyan".to_owned()])));
        assert!(can_moderate(&Caller { manages_server: true, ..caller(&[]) }));
        assert!(can_moderate(&Caller { administrator: true, ..caller(&[]) }));
    }
}
