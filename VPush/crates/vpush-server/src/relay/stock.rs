//! Taking stock: what a relay held before the watch of a key or a group
//! began there is old, and is not pushed.
//!
//! A key or a group the relay is asked about for the first time is fresh
//! until the relay says that it has sent all it had stored. What comes for
//! it before that is taken stock of. From then on it is known on this
//! relay, and what comes for it is new.

use std::collections::{BTreeSet, HashMap};

use super::plan::{subscriptions, Sub};
use crate::pipeline::classify::Subject;
use crate::store::{RelayPlan, WatchKind};

/// A key whose direct messages are watched, or a group.
pub type Target = (WatchKind, String);

/// What is known and what is fresh on one relay.
pub struct Stock {
    /// What stock was taken of.
    known: BTreeSet<Target>,
    /// By request: the keys and groups the relay is asked about for the
    /// first time, until it has sent what it had stored for the request.
    fresh: HashMap<String, BTreeSet<Target>>,
    /// How many times the requests were made anew.
    generation: u64,
}

impl Stock {
    pub fn new(known: impl IntoIterator<Item = Target>) -> Self {
        Self {
            known: known.into_iter().collect(),
            fresh: HashMap::new(),
            generation: 0,
        }
    }

    /// The requests for `plan`, which take the place of all made before,
    /// and the keys and groups that were known and are no longer watched.
    ///
    /// Those are known no more. While nobody watched, the relay went on
    /// taking events for them; when they are watched again those events
    /// are old, and only a key that is fresh has its old events told from
    /// its new ones.
    pub fn replan(
        &mut self,
        plan: &RelayPlan,
        now: u64,
        last_alive: Option<u64>,
    ) -> (Vec<Sub>, Vec<Target>) {
        // The lists of a plan are sorted.
        let watched = |(kind, target): &Target| match kind {
            WatchKind::Dm => plan.dm.binary_search(target).is_ok(),
            WatchKind::Group => plan.groups.binary_search(target).is_ok(),
        };
        let left: Vec<Target> = self.known.iter().filter(|t| !watched(t)).cloned().collect();
        for target in &left {
            self.known.remove(target);
        }

        self.generation += 1;
        let subs = subscriptions(plan, now, last_alive, self.generation);
        self.fresh = subs
            .iter()
            .filter_map(|sub| {
                let fresh: BTreeSet<Target> = sub
                    .targets
                    .iter()
                    .map(|target| (sub.kind, target.clone()))
                    .filter(|target| !self.known.contains(target))
                    .collect();
                (!fresh.is_empty()).then(|| (sub.id.clone(), fresh))
            })
            .collect();
        (subs, left)
    }

    /// Is the event of a key or a group stock has not been taken of yet.
    ///
    /// Whichever request brought it: an event still on its way from a
    /// request that was replaced is as old as one the new request brings.
    pub fn is_fresh(&self, subject: &Subject) -> bool {
        let fresh = |target: Target| self.fresh.values().any(|set| set.contains(&target));
        match subject {
            Subject::Dm { recipients } => recipients
                .iter()
                .all(|p| fresh((WatchKind::Dm, p.clone()))),
            Subject::Group { id } => fresh((WatchKind::Group, id.clone())),
        }
    }

    /// The relay sent all it had stored for a request. Returns what was
    /// fresh in it: stock of these is taken.
    ///
    /// Nothing, for a request of a generation before this one. The relay
    /// was still answering it when it was replaced, and the request that
    /// replaced it has an end of its own to wait for.
    pub fn stored_is_over(&mut self, sub: &str) -> Vec<Target> {
        self.fresh
            .remove(sub)
            .map(|fresh| fresh.into_iter().collect())
            .unwrap_or_default()
    }

    /// Stock of these was taken, and written down.
    pub fn taken(&mut self, targets: Vec<Target>) {
        self.known.extend(targets);
    }

    /// How many keys and groups are fresh.
    pub fn fresh(&self) -> usize {
        self.fresh.values().map(BTreeSet::len).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_790_000_000;

    fn plan(dm: &[&str], groups: &[&str]) -> RelayPlan {
        RelayPlan {
            url: "wss://relay.example.org".into(),
            dm: dm.iter().map(|t| t.to_string()).collect(),
            groups: groups.iter().map(|t| t.to_string()).collect(),
        }
    }

    fn dm(key: &str) -> Target {
        (WatchKind::Dm, key.to_string())
    }

    fn group(id: &str) -> Target {
        (WatchKind::Group, id.to_string())
    }

    fn to(keys: &[&str]) -> Subject {
        Subject::Dm {
            recipients: keys.iter().map(|k| k.to_string()).collect(),
        }
    }

    fn of(id: &str) -> Subject {
        Subject::Group { id: id.to_string() }
    }

    /// Makes the requests anew, and returns their ids.
    fn ask(stock: &mut Stock, plan: &RelayPlan) -> Vec<String> {
        let (subs, _) = stock.replan(plan, NOW, None);
        subs.into_iter().map(|sub| sub.id).collect()
    }

    #[test]
    fn what_comes_before_the_relay_has_sent_what_it_had_is_old() {
        let mut stock = Stock::new([dm("bob")]);
        let ids = ask(&mut stock, &plan(&["alice", "bob"], &["g1"]));
        assert_eq!(ids, ["dm-0-g1", "group-0-g1"]);

        assert!(stock.is_fresh(&to(&["alice"])));
        assert!(stock.is_fresh(&of("g1")));
        assert!(!stock.is_fresh(&to(&["bob"])), "stock of bob was taken before");
        assert!(!stock.is_fresh(&to(&["carol"])), "nobody watches carol");
        assert!(!stock.is_fresh(&to(&["alice", "bob"])), "new to bob, so not old");

        let taken = stock.stored_is_over("dm-0-g1");
        assert_eq!(taken, [dm("alice")]);
        stock.taken(taken);
        assert!(!stock.is_fresh(&to(&["alice"])), "from now on her messages are new");
        assert!(stock.is_fresh(&of("g1")), "the group has an end of its own");
        assert_eq!(stock.fresh(), 1);
    }

    #[test]
    fn a_late_end_of_a_request_that_was_replaced_is_not_the_end_of_the_new_one() {
        let mut stock = Stock::new([]);
        let first = ask(&mut stock, &plan(&["alice"], &[]));
        // The plan changes while the relay is still sending what it had.
        let second = ask(&mut stock, &plan(&["alice", "bob"], &[]));
        assert_ne!(first, second);

        // The end of the first request comes after the second was made.
        assert!(stock.stored_is_over(&first[0]).is_empty());
        assert!(stock.is_fresh(&to(&["alice"])), "what the second request brings is still old");

        assert_eq!(stock.stored_is_over(&second[0]), [dm("alice"), dm("bob")]);
    }

    #[test]
    fn what_was_not_written_down_is_asked_about_as_fresh_again() {
        let mut stock = Stock::new([]);
        let ids = ask(&mut stock, &plan(&["alice"], &[]));
        // The end came, and the store failed: `taken` is not called.
        assert_eq!(stock.stored_is_over(&ids[0]), [dm("alice")]);
        ask(&mut stock, &plan(&["alice"], &[]));
        assert!(stock.is_fresh(&to(&["alice"])));
    }

    #[test]
    fn what_leaves_the_plan_is_forgotten_and_is_fresh_when_it_comes_back() {
        let mut stock = Stock::new([dm("alice"), dm("bob"), group("g1")]);

        let (_, left) = stock.replan(&plan(&["bob"], &["g1"]), NOW, None);
        assert_eq!(left, [dm("alice")]);
        assert_eq!(stock.fresh(), 0, "those who stay are known as before");

        let (_, left) = stock.replan(&plan(&["bob"], &[]), NOW, None);
        assert_eq!(left, [group("g1")]);

        // Alice and the group are back. What the relay took for them in
        // the meantime is old.
        let (_, left) = stock.replan(&plan(&["alice", "bob"], &["g1"]), NOW, None);
        assert!(left.is_empty());
        assert!(stock.is_fresh(&to(&["alice"])));
        assert!(stock.is_fresh(&of("g1")));
        assert!(!stock.is_fresh(&to(&["bob"])));
    }
}
