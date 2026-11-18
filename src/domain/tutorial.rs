//! The interactive tutorial, Bot3's way: one task at a time, and the bot moves on only once it sees
//! the task done. Pure data: what each step asks, and what counts as doing it.
//!
//! ```text
//!   /tutorial ─► a thread of your own ─► step 1: "Type /mypage" ─► you do it ─► ✅ ─► step 2 …
//!                                          [Show again] [Skip] [Stop]
//! ```
//!
//! Progress is kept by step id, so adding a step never moves anyone.

/// Something a person did that a step may be waiting for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Did {
    /// A slash command finished, by name: "mypage", "z_moderator_sp".
    Command(String),
    /// Picked who they play on /panel.
    PanelPick,
    /// Typed .set<slot>.
    DotSet,
    /// Gave a character a prefix on their page.
    PrefixSet,
    /// Posted as a character; `expression`: with one; `npc`: as one of the cast.
    Proxied { expression: bool, npc: bool },
    /// Edited a post made as a character.
    ProxyEdited,
    /// Deleted a post made as a character.
    ProxyDeleted,
    /// Pruned messages.
    Pruned,
    /// Bought something in the shop.
    Bought,
    /// Changed the shop's shelf or a player's inventory, as staff.
    ShopChanged,
}

/// What finishes a step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Expect {
    Command(&'static str),
    PanelPick,
    DotSet,
    PrefixSet,
    Proxied,
    ProxiedWithExpression,
    ProxiedAsNpc,
    ProxyEdited,
    ProxyDeleted,
    Pruned,
    Bought,
    ShopChanged,
}

impl Expect {
    pub(crate) fn met_by(self, did: &Did) -> bool {
        match (self, did) {
            (Self::Command(name), Did::Command(done)) => name == done,
            (Self::PanelPick, Did::PanelPick | Did::DotSet) => true,
            (Self::DotSet, Did::DotSet) | (Self::PrefixSet, Did::PrefixSet) | (Self::ProxyEdited, Did::ProxyEdited) => true,
            (Self::ProxyDeleted, Did::ProxyDeleted) | (Self::Pruned, Did::Pruned) => true,
            (Self::Bought, Did::Bought) | (Self::ShopChanged, Did::ShopChanged) => true,
            (Self::Proxied, Did::Proxied { npc: false, .. }) => true,
            (Self::ProxiedWithExpression, Did::Proxied { expression: true, npc: false }) => true,
            (Self::ProxiedAsNpc, Did::Proxied { npc: true, .. }) => true,
            _ => false,
        }
    }
}

pub(crate) struct Step {
    pub(crate) id: &'static str,
    pub(crate) title: &'static str,
    /// What to do, as one or two lines.
    pub(crate) task: &'static str,
    pub(crate) expect: Expect,
}

/// Who a track is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Track {
    Players,
    Staff,
}

impl Track {
    pub(crate) const ALL: [Self; 2] = [Self::Players, Self::Staff];

    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::Players => "players",
            Self::Staff => "staff",
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Players => "Players: your page, slots, posting as your character, the shop",
            Self::Staff => "Staff: the admin pages, NPCs, Spirit Points, the shop, clean-up",
        }
    }

    pub(crate) fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.key() == key)
    }

    pub(crate) const fn steps(self) -> &'static [Step] {
        match self {
            Self::Players => PLAYERS,
            Self::Staff => STAFF,
        }
    }

    /// The step after `id`; None when `id` was the last.
    pub(crate) fn after(self, id: &str) -> Option<&'static Step> {
        let steps = self.steps();
        let at = steps.iter().position(|s| s.id == id)?;
        steps.get(at + 1)
    }

    pub(crate) fn step(self, id: &str) -> Option<&'static Step> {
        self.steps().iter().find(|s| s.id == id)
    }

    /// Where a step sits: (1-based number, how many).
    pub(crate) fn place(self, id: &str) -> (usize, usize) {
        let steps = self.steps();
        (steps.iter().position(|s| s.id == id).map_or(0, |at| at + 1), steps.len())
    }
}

const PLAYERS: &[Step] = &[
    Step { id: "page", title: "Your page", task: "Type **`/mypage`**. The bot DMs you a link to your page: your characters, who you play, your Spirit Points.", expect: Expect::Command("mypage") },
    Step { id: "panel", title: "Your slots", task: "Type **`/panel`**, then press a number to play that character.", expect: Expect::PanelPick },
    Step { id: "dotset", title: "The quick switch", task: "Type **`.set1`** to play slot 1 (`.set2` for slot 2, and so on).", expect: Expect::DotSet },
    Step { id: "prefix", title: "A prefix", task: "On your page, under a character's **Posting**, give them a prefix, like `nova`, and press **Save posting**.", expect: Expect::PrefixSet },
    Step { id: "proxy", title: "Post as them", task: "Type **`<prefix>: Hello!`** here, with your prefix. The bot posts it under their name and picture.", expect: Expect::Proxied },
    Step { id: "expression", title: "An expression", task: "On your page, add an expression: a name like `happy` and an image link. Then post **`<prefix>:happy Hi!`**.", expect: Expect::ProxiedWithExpression },
    Step { id: "edit", title: "Edit a post", task: "React ✏️ to one of your posts, press **Edit**, and change the text.", expect: Expect::ProxyEdited },
    Step { id: "delete", title: "Delete a post", task: "React ❌ to one of your posts: it is gone, every part of it.", expect: Expect::ProxyDeleted },
    Step { id: "profile", title: "Show a card", task: "Type **`/profile`**: everyone in the channel sees your character's card.", expect: Expect::Command("profile") },
    Step { id: "shop", title: "The shop", task: "On your page, open **Shop** and buy something: 🧑‍💻 Tom (MySpace) is free. It lands in your **Inventory**.", expect: Expect::Bought },
    Step { id: "help", title: "Look it up", task: "Type **`/help`**, or `/help posting`, whenever you forget.", expect: Expect::Command("help") },
];

const STAFF: &[Step] = &[
    Step { id: "admin", title: "The admin pages", task: "Type **`/z_moderator_admin`**. The link comes by DM: players, characters, NPCs, McGuffins, Dragon Balls, worlds.", expect: Expect::Command("z_moderator_admin") },
    Step { id: "npc", title: "Post as an NPC", task: "On the admin pages, **NPCs**: add one with a prefix. Then post here as it: **`<prefix>: Greetings.`** Any staff member may post as any NPC.", expect: Expect::ProxiedAsNpc },
    Step { id: "sp", title: "Spirit Points", task: "Type **`/z_moderator_sp action:history player:@you`**, naming yourself.", expect: Expect::Command("z_moderator_sp") },
    Step { id: "shop", title: "The shop", task: "On the admin pages, **Shop**: add an item or change one. Or open a player and set what they own, under **Inventory**.", expect: Expect::ShopChanged },
    Step { id: "prune", title: "Clean up", task: "Type **`.prune 1`** here: it deletes the message before it, then itself.", expect: Expect::Pruned },
    Step { id: "help", title: "Look it up", task: "Type **`/help`**: staff see staff pages too. Try `/help sp give`.", expect: Expect::Command("help") },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_have_unique_ids_and_follow_each_other() {
        for track in Track::ALL {
            let steps = track.steps();
            let mut ids: Vec<_> = steps.iter().map(|s| s.id).collect();
            ids.dedup();
            assert_eq!(ids.len(), steps.len());
            assert_eq!(track.after(steps[0].id).map(|s| s.id), steps.get(1).map(|s| s.id));
            assert!(track.after(steps[steps.len() - 1].id).is_none());
        }
    }

    #[test]
    fn a_step_is_met_only_by_what_it_asks() {
        assert!(Expect::Command("mypage").met_by(&Did::Command("mypage".to_owned())));
        assert!(!Expect::Command("mypage").met_by(&Did::Command("profile".to_owned())));
        assert!(Expect::PanelPick.met_by(&Did::DotSet));
        assert!(!Expect::ProxiedWithExpression.met_by(&Did::Proxied { expression: false, npc: false }));
        assert!(Expect::ProxiedWithExpression.met_by(&Did::Proxied { expression: true, npc: false }));
        assert!(!Expect::Proxied.met_by(&Did::Proxied { expression: false, npc: true }));
        assert!(Expect::ProxiedAsNpc.met_by(&Did::Proxied { expression: false, npc: true }));
        assert!(Expect::Bought.met_by(&Did::Bought) && !Expect::Bought.met_by(&Did::ShopChanged));
    }
}
