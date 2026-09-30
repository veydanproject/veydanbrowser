//! How often a device is woken.
//!
//! The first event about a chat is pushed at once. What follows within the
//! window is counted, not pushed, and when the window ends one more push
//! says how many there were, and names the last of them. So a burst of
//! fifty messages wakes the phone twice, and none of the fifty goes untold.

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

/// An event as the push at the end of a window names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    pub event_id: String,
    pub relay: String,
}

/// What a window ended with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Due {
    /// Events counted since the last push.
    pub count: u32,
    /// The last of them.
    pub last: Named,
}

#[derive(Debug)]
struct State {
    last: Instant,
    /// Events counted since the last push, and the last of them.
    waiting: Option<Due>,
}

impl State {
    fn count(&self) -> u32 {
        self.waiting.as_ref().map_or(0, |w| w.count)
    }

    fn counted(&mut self, event: Named) {
        match &mut self.waiting {
            Some(due) => {
                due.count += 1;
                due.last = event;
            }
            None => self.waiting = Some(Due { count: 1, last: event }),
        }
    }
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

    pub fn event(&self, key: &Key, now: Instant, event: Named) -> Verdict {
        let mut states = self.states.lock().unwrap();
        if states.len() >= Self::SWEEP_AT {
            let window = self.window;
            states.retain(|_, s| s.waiting.is_some() || now.duration_since(s.last) < window);
        }
        match states.get_mut(key) {
            Some(state) if now.duration_since(state.last) < self.window => {
                let first = state.waiting.is_none();
                state.counted(event);
                Verdict::Later {
                    wake_in: first.then(|| self.window - now.duration_since(state.last)),
                }
            }
            Some(state) if state.count() > 0 => {
                // The window is over and its push has not gone out yet: this
                // event goes with it.
                state.counted(event);
                Verdict::Later { wake_in: None }
            }
            _ => {
                states.insert(key.clone(), State { last: now, waiting: None });
                Verdict::Now
            }
        }
    }

    /// The window of `key` ended: what the push is for. `None` when there
    /// is nothing to push.
    pub fn due(&self, key: &Key, now: Instant) -> Option<Due> {
        let mut states = self.states.lock().unwrap();
        let state = states.get_mut(key)?;
        let due = state.waiting.take()?;
        state.last = now;
        Some(due)
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

    fn named(n: u64) -> Named {
        Named { event_id: format!("e{n}"), relay: "wss://r".into() }
    }

    /// An event of the chat `chat`, numbered `n`.
    fn event(t: &Throttle, chat: &str, at: Instant, n: u64) -> Verdict {
        t.event(&key(chat), at, named(n))
    }

    /// How many the push at the end is for, and which event it names.
    fn due(t: &Throttle, chat: &str, at: Instant) -> Option<(u32, String)> {
        t.due(&key(chat), at).map(|d| (d.count, d.last.event_id))
    }

    #[test]
    fn the_first_is_pushed_at_once_and_the_burst_is_counted() {
        let t = Throttle::new(WINDOW);
        let start = Instant::now();
        assert_eq!(event(&t, "dm", start, 1), Verdict::Now);
        assert_eq!(
            event(&t, "dm", start + secs(5), 2),
            Verdict::Later { wake_in: Some(secs(15)) }
        );
        for i in 6..10 {
            assert_eq!(event(&t, "dm", start + secs(i), i), Verdict::Later { wake_in: None });
        }
        assert_eq!(due(&t, "dm", start + WINDOW), Some((5, "e9".into())), "the last one is named");
        assert_eq!(due(&t, "dm", start + WINDOW), None, "told once");
    }

    #[test]
    fn the_push_at_the_end_opens_a_window_of_its_own() {
        let t = Throttle::new(WINDOW);
        let start = Instant::now();
        event(&t, "dm", start, 1);
        event(&t, "dm", start + secs(1), 2);
        assert_eq!(due(&t, "dm", start + WINDOW), Some((1, "e2".into())));

        assert!(matches!(
            event(&t, "dm", start + WINDOW + secs(1), 3),
            Verdict::Later { wake_in: Some(_) }
        ));
        assert_eq!(due(&t, "dm", start + WINDOW * 2), Some((1, "e3".into())));
    }

    #[test]
    fn after_a_quiet_window_the_next_is_pushed_at_once() {
        let t = Throttle::new(WINDOW);
        let start = Instant::now();
        assert_eq!(event(&t, "dm", start, 1), Verdict::Now);
        assert_eq!(due(&t, "dm", start + WINDOW), None);
        assert_eq!(event(&t, "dm", start + WINDOW + secs(1), 2), Verdict::Now);
    }

    #[test]
    fn chats_and_devices_do_not_hold_each_other_back() {
        let t = Throttle::new(WINDOW);
        let now = Instant::now();
        assert_eq!(event(&t, "dm", now, 1), Verdict::Now);
        assert_eq!(event(&t, "group:1", now, 1), Verdict::Now);
        assert_eq!(
            t.event(&("owner".into(), "tablet".into(), "dm".into()), now, named(1)),
            Verdict::Now
        );
        assert_eq!(
            t.event(&("other".into(), "phone".into(), "dm".into()), now, named(1)),
            Verdict::Now
        );
    }

    #[test]
    fn an_event_that_comes_late_for_the_window_goes_with_its_push() {
        let t = Throttle::new(WINDOW);
        let start = Instant::now();
        event(&t, "dm", start, 1);
        event(&t, "dm", start + secs(1), 2);
        // The one who was to come back is a little late.
        assert_eq!(event(&t, "dm", start + WINDOW + secs(1), 3), Verdict::Later { wake_in: None });
        assert_eq!(due(&t, "dm", start + WINDOW + secs(2)), Some((2, "e3".into())));
    }

    #[test]
    fn what_did_its_work_is_dropped_and_what_waits_is_kept() {
        let t = Throttle::new(WINDOW);
        let start = Instant::now();
        for i in 0..Throttle::SWEEP_AT {
            event(&t, &format!("group:{i}"), start, 1);
        }
        event(&t, "group:0", start + secs(1), 2);
        assert_eq!(t.len(), Throttle::SWEEP_AT);

        event(&t, "dm", start + WINDOW + secs(1), 1);
        assert_eq!(t.len(), 2, "the one that waits, and the new one");
        assert_eq!(due(&t, "group:0", start + WINDOW + secs(1)), Some((1, "e2".into())));
    }
}
