//! What happened a moment ago, counted by key: the signatures that were
//! used, the test pushes that were asked for.
//!
//! Whoever can make the server remember a thing can ask it to remember a
//! million of them. So the count is kept for a bounded number of keys, and
//! when that many are remembered a new one is refused, not added.

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::time::{Duration, Instant};

/// Why a key is not taken. Both come with the time after which to ask again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// The key was taken as often as it may be.
    TooOften(Duration),
    /// As many keys are remembered as may be.
    Full(Duration),
}

impl Refused {
    pub fn wait(self) -> Duration {
        match self {
            Self::TooOften(wait) | Self::Full(wait) => wait,
        }
    }
}

/// How often each key was taken in the last `window`.
pub struct Recent<K> {
    window: Duration,
    /// How often one key may be taken within the window.
    per_key: usize,
    /// How many keys are remembered at one time.
    max_keys: usize,
    /// Every take still within the window, the oldest first. What has run
    /// out is at the front, so forgetting it never looks at the rest.
    order: VecDeque<(Instant, K)>,
    /// The same takes by key, the oldest first.
    by_key: HashMap<K, VecDeque<Instant>>,
}

impl<K: Hash + Eq + Clone> Recent<K> {
    pub fn new(window: Duration, per_key: usize, max_keys: usize) -> Self {
        Self {
            window,
            per_key,
            max_keys,
            order: VecDeque::new(),
            by_key: HashMap::new(),
        }
    }

    /// Counts one more take of `key`, or says why not.
    pub fn take(&mut self, key: K, now: Instant) -> Result<(), Refused> {
        self.forget(now);
        // Until the take made at `since` leaves the window.
        let wait = |since: Option<&Instant>| match since {
            Some(at) => self.window.saturating_sub(now.duration_since(*at)),
            None => self.window,
        };
        let taken = self.by_key.get(&key);
        if taken.map_or(0, VecDeque::len) >= self.per_key {
            return Err(Refused::TooOften(wait(taken.and_then(VecDeque::front))));
        }
        if taken.is_none() && self.by_key.len() >= self.max_keys {
            let oldest = self.order.front().map(|(at, _)| at);
            return Err(Refused::Full(wait(oldest)));
        }
        self.by_key.entry(key.clone()).or_default().push_back(now);
        self.order.push_back((now, key));
        Ok(())
    }

    /// Drops the takes that have left the window.
    fn forget(&mut self, now: Instant) {
        while self.order.front().is_some_and(|(at, _)| now.duration_since(*at) >= self.window) {
            let Some((_, key)) = self.order.pop_front() else { break };
            // The oldest take of all is the oldest take of its key.
            if let Some(times) = self.by_key.get_mut(&key) {
                times.pop_front();
                if times.is_empty() {
                    self.by_key.remove(&key);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: Duration = Duration::from_secs(3600);

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn a_key_is_taken_as_often_as_it_may_be_and_no_more() {
        let start = Instant::now();
        let mut recent = Recent::new(HOUR, 2, 100);
        recent.take("a", start).unwrap();
        recent.take("a", start + secs(600)).unwrap();
        assert_eq!(
            recent.take("a", start + secs(900)),
            Err(Refused::TooOften(secs(2700))),
            "until the first of the two is an hour old"
        );
        recent.take("b", start + secs(900)).unwrap();

        recent.take("a", start + secs(3600)).unwrap();
        assert_eq!(recent.take("a", start + secs(3601)), Err(Refused::TooOften(secs(599))));
    }

    #[test]
    fn what_left_the_window_is_forgotten() {
        let start = Instant::now();
        let mut recent = Recent::new(HOUR, 1, 100);
        for (i, key) in ["a", "b", "c"].into_iter().enumerate() {
            recent.take(key, start + secs(i as u64)).unwrap();
        }
        assert_eq!(recent.by_key.len(), 3);
        recent.take("d", start + secs(3601)).unwrap();
        assert_eq!(recent.by_key.len(), 2, "`c` and `d` are within the hour");
    }

    #[test]
    fn a_full_memory_refuses_new_keys_and_does_not_grow() {
        let start = Instant::now();
        let mut recent = Recent::new(HOUR, 2, 3);
        for (i, key) in ["a", "b", "c"].into_iter().enumerate() {
            recent.take(key, start + secs(i as u64 * 60)).unwrap();
        }
        assert_eq!(recent.take("d", start + secs(600)), Err(Refused::Full(secs(3000))));
        assert_eq!(recent.by_key.len(), 3);
        // A key that is remembered is still counted.
        recent.take("b", start + secs(600)).unwrap();

        // `a` runs out, and there is room for one more.
        recent.take("d", start + secs(3600)).unwrap();
        assert_eq!(recent.take("e", start + secs(3600)), Err(Refused::Full(secs(60))));
    }

    #[test]
    fn nothing_may_be_taken_when_the_limit_is_zero() {
        let mut recent = Recent::new(HOUR, 0, 100);
        assert_eq!(recent.take("a", Instant::now()), Err(Refused::TooOften(HOUR)));
        assert_eq!(recent.by_key.len(), 0);
    }
}
