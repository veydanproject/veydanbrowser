//! How often a device is woken.
//!
//! The first event about a chat is pushed at once. What follows within the
//! window is counted, not pushed, and when the window ends one more push
//! says how many there were. So a burst of fifty messages wakes the phone
//! twice, and none of the fifty goes untold.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A device and what the push is about: `dm`, `group:<id>`.
pub type Key = (String, String, String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Push now.
    Now,
    /// Counted. `wake_in` is set for the first one counted: somebody has to
    /// come back after that long and ask [`Throttle::due`].
    Later { wake_in: Option<Duration> },
}

#[derive(Debug)]
struct State {
    last: Instant,
    /// Events counted since the last push.
    waiting: u32,
}

pub struct Throttle {
    window: Duration,
    states: Mutex<HashMap<Key, State>>,
}

impl Throttle {
    /// More states than this, and the ones that did their work are dropped.
    const SWEEP_AT: usize = 10_000;

    pub fn new(window: Duration) -> Self {
        Self {
            window,
            states: Mutex::new(HashMap::new()),
        }
    }

    pub fn event(&self, key: &Key, now: Instant) -> Verdict {
        let mut states = self.states.lock().unwrap();
        if states.len() >= Self::SWEEP_AT {
            let window = self.window;
            states.retain(|_, s| s.waiting > 0 || now.duration_since(s.last) < window);
        }
        match states.get_mut(key) {
            Some(state) if now.duration_since(state.last) < self.window => {
                state.waiting += 1;
                let first = state.waiting == 1;
                Verdict::Later {
                    wake_in: first.then(|| self.window - now.duration_since(state.last)),
                }
            }
            Some(state) if state.waiting > 0 => {
                // The window is over and its push has not gone out yet: this
                // event goes with it.
                state.waiting += 1;
                Verdict::Later { wake_in: None }
            }
            _ => {
                states.insert(key.clone(), State { last: now, waiting: 0 });
                Verdict::Now
            }
        }
    }

    /// The window of `key` ended: how many events the push is for. Zero when
    /// there is nothing to push.
    pub fn due(&self, key: &Key, now: Instant) -> u32 {
        let mut states = self.states.lock().unwrap();
        match states.get_mut(key) {
            Some(state) if state.waiting > 0 => {
                let count = state.waiting;
                state.waiting = 0;
                state.last = now;
                count
            }
            _ => 0,
        }
    }

    pub fn len(&self) -> usize {
        self.states.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Duration = Duration::from_secs(20);

    fn key(chat: &str) -> Key {
        ("owner".into(), "phone".into(), chat.into())
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn the_first_is_pushed_at_once_and_the_burst_is_counted() {
        let t = Throttle::new(WINDOW);
        let start = Instant::now();
        assert_eq!(t.event(&key("dm"), start), Verdict::Now);
        assert_eq!(
            t.event(&key("dm"), start + secs(5)),
            Verdict::Later { wake_in: Some(secs(15)) }
        );
        for i in 6..10 {
            assert_eq!(t.event(&key("dm"), start + secs(i)), Verdict::Later { wake_in: None });
        }
        assert_eq!(t.due(&key("dm"), start + WINDOW), 5);
        assert_eq!(t.due(&key("dm"), start + WINDOW), 0, "told once");
    }

    #[test]
    fn the_push_at_the_end_opens_a_window_of_its_own() {
        let t = Throttle::new(WINDOW);
        let start = Instant::now();
        t.event(&key("dm"), start);
        t.event(&key("dm"), start + secs(1));
        assert_eq!(t.due(&key("dm"), start + WINDOW), 1);

        assert!(matches!(
            t.event(&key("dm"), start + WINDOW + secs(1)),
            Verdict::Later { wake_in: Some(_) }
        ));
        assert_eq!(t.due(&key("dm"), start + WINDOW * 2), 1);
    }

    #[test]
    fn after_a_quiet_window_the_next_is_pushed_at_once() {
        let t = Throttle::new(WINDOW);
        let start = Instant::now();
        assert_eq!(t.event(&key("dm"), start), Verdict::Now);
        assert_eq!(t.due(&key("dm"), start + WINDOW), 0);
        assert_eq!(t.event(&key("dm"), start + WINDOW + secs(1)), Verdict::Now);
    }

    #[test]
    fn chats_and_devices_do_not_hold_each_other_back() {
        let t = Throttle::new(WINDOW);
        let now = Instant::now();
        assert_eq!(t.event(&key("dm"), now), Verdict::Now);
        assert_eq!(t.event(&key("group:1"), now), Verdict::Now);
        assert_eq!(t.event(&("owner".into(), "tablet".into(), "dm".into()), now), Verdict::Now);
        assert_eq!(t.event(&("other".into(), "phone".into(), "dm".into()), now), Verdict::Now);
    }

    #[test]
    fn an_event_that_comes_late_for_the_window_goes_with_its_push() {
        let t = Throttle::new(WINDOW);
        let start = Instant::now();
        t.event(&key("dm"), start);
        t.event(&key("dm"), start + secs(1));
        // The one who was to come back is a little late.
        assert_eq!(t.event(&key("dm"), start + WINDOW + secs(1)), Verdict::Later { wake_in: None });
        assert_eq!(t.due(&key("dm"), start + WINDOW + secs(2)), 2);
    }

    #[test]
    fn what_did_its_work_is_dropped_and_what_waits_is_kept() {
        let t = Throttle::new(WINDOW);
        let start = Instant::now();
        for i in 0..Throttle::SWEEP_AT {
            t.event(&key(&format!("group:{i}")), start);
        }
        t.event(&key("group:0"), start + secs(1));
        assert_eq!(t.len(), Throttle::SWEEP_AT);

        t.event(&key("dm"), start + WINDOW + secs(1));
        assert_eq!(t.len(), 2, "the one that waits, and the new one");
        assert_eq!(t.due(&key("group:0"), start + WINDOW + secs(1)), 1);
    }
}
