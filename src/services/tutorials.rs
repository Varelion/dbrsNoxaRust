//! The interactive tutorial's progress: one step at a time, moved on only by doing what it asks.
//!
//! ```text
//!   start(user, Players, thread) ─► step 1
//!   did(user, Did::Command("mypage")) ─► Some(Moved { done: step 1, next: step 2 }) · None: not what it waits for
//!   skip(user) ─► the next step · stop(user) ─► gone
//! ```

use anyhow::Result;

use crate::domain::tutorial::{Did, Step, Track};
use crate::store::Store;
use crate::store::tutorials::{self as repo, TutorialRow};

use super::Clock;

/// Where someone is: their track, their step, and the thread it runs in.
#[derive(Clone, Copy)]
pub(crate) struct At {
    pub(crate) track: Track,
    pub(crate) step: &'static Step,
    pub(crate) thread: Option<u64>,
}

/// A step finished: the one done, and the one after (None: the track is done, and over).
#[derive(Clone, Copy)]
pub(crate) struct Moved {
    pub(crate) track: Track,
    pub(crate) done: &'static Step,
    pub(crate) next: Option<&'static Step>,
    pub(crate) thread: Option<u64>,
}

pub(crate) struct Tutorials<'a> {
    store: &'a Store,
    clock: &'a Clock,
}

impl<'a> Tutorials<'a> {
    pub(crate) const fn new(store: &'a Store, clock: &'a Clock) -> Self {
        Self { store, clock }
    }

    pub(crate) fn start(&self, user: u64, track: Track, thread: Option<u64>) -> Result<At> {
        let step = &track.steps()[0];
        let row = TutorialRow { user_id: user, track: track.key().to_owned(), step: step.id.to_owned(), thread_id: thread };
        let at = self.clock.now();
        self.store.write(|tx| repo::start(tx, &row, at))?;
        Ok(At { track, step, thread })
    }

    /// Where someone is; None when they run no tutorial (or a step was removed under them).
    pub(crate) fn at(&self, user: u64) -> Result<Option<At>> {
        let Some(row) = self.store.read(|c| repo::get(c, user))? else {
            return Ok(None);
        };
        let Some(track) = Track::from_key(&row.track) else {
            return Ok(None);
        };
        Ok(track.step(&row.step).map(|step| At { track, step, thread: row.thread_id }))
    }

    /// What they did, checked against the step waiting on them.
    pub(crate) fn did(&self, user: u64, did: &Did) -> Result<Option<Moved>> {
        let Some(at) = self.at(user)? else {
            return Ok(None);
        };
        if !at.step.expect.met_by(did) {
            return Ok(None);
        }
        self.advance(user, at).map(Some)
    }

    pub(crate) fn skip(&self, user: u64) -> Result<Option<Moved>> {
        let Some(at) = self.at(user)? else {
            return Ok(None);
        };
        self.advance(user, at).map(Some)
    }

    pub(crate) fn stop(&self, user: u64) -> Result<()> {
        self.store.write(|tx| repo::stop(tx, user))
    }

    fn advance(&self, user: u64, at: At) -> Result<Moved> {
        let next = at.track.after(at.step.id);
        self.store.write(|tx| match next {
            Some(step) => repo::set_step(tx, user, step.id),
            None => repo::stop(tx, user),
        })?;
        Ok(Moved { track: at.track, done: at.step, next, thread: at.thread })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::Services;

    #[test]
    fn a_step_moves_on_only_when_done_and_the_last_ends_the_tutorial() {
        let services = Services::memory(0);
        let tutorials = services.tutorials();
        assert_eq!(tutorials.start(1, Track::Staff, Some(77)).unwrap().step.id, "admin");

        assert!(tutorials.did(1, &Did::Command("help".to_owned())).unwrap().is_none());
        let moved = tutorials.did(1, &Did::Command("z_moderator_admin".to_owned())).unwrap().unwrap();
        assert_eq!((moved.done.id, moved.next.map(|s| s.id), moved.thread), ("admin", Some("npc"), Some(77)));
        assert!(tutorials.did(2, &Did::Pruned).unwrap().is_none());

        while tutorials.skip(1).unwrap().is_some_and(|m| m.next.is_some()) {}
        assert!(tutorials.at(1).unwrap().is_none());
    }
}
