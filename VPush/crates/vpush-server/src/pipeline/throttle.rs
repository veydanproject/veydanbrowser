//! How often a device is woken.
//!
//! The first event about a chat is pushed at once. What follows within the
//! window is counted, not pushed, and when the window ends one more push
//! says how many there were, and names the last of them. So a burst of
//! fifty messages wakes the phone twice, and none of the fifty goes untold.
//!
//! That holds a chat back, and a device has many chats. So a device is also
//! pushed to no more than so many times within a window, whatever the pushes
//! are about. What comes over that is counted, and one push of the type
//! `sync` says how many there were: the device shows the number and takes
//! the messages from its relays.

use std::collections::{HashMap, VecDeque};
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

/// A device: its owner and its id.
pub type Device = (String, String);

/// Whether a device may be pushed to now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Room {
    /// Push.
    Yes,
    /// The device was pushed to as often as it may be: what the push stood
    /// for is counted. `wake_in` is set for the first one counted: somebody
    /// has to come back after that long and ask [`DeviceCap::due`].
    No { wake_in: Option<Duration> },
}

#[derive(Debug, Default)]
struct Pushed {
    /// When the device was pushed to within the window, the oldest first.
    at: VecDeque<Instant>,
    /// Events that came over the cap since the last `sync` push.
    over: u32,
}

pub struct DeviceCap {
    /// Pushes to one device within the window.
    cap: usize,
    window: Duration,
    states: Mutex<HashMap<Device, Pushed>>,
}

impl DeviceCap {
    /// More states than this, and the ones that did their work are dropped.
    const SWEEP_AT: usize = 10_000;

    pub fn new(cap: usize, window: Duration) -> Self {
        Self {
            cap,
            window,
            states: Mutex::new(HashMap::new()),
        }
    }

    /// A push to `device` that stands for `events` events: may it go now.
    ///
    /// The one who comes back for the counted comes a whole window after
    /// the first of them, so a device gets one `sync` push in a window and
    /// no more.
    pub fn push(&self, device: &Device, now: Instant, events: u32) -> Room {
        let mut states = self.states.lock().unwrap();
        let window = self.window;
        let recent = |at: &Instant| now.duration_since(*at) < window;
        if states.len() >= Self::SWEEP_AT {
            states.retain(|_, s| s.over > 0 || s.at.back().is_some_and(recent));
        }
        let state = states.entry(device.clone()).or_default();
        while state.at.front().is_some_and(|at| !recent(at)) {
            state.at.pop_front();
        }
        if state.at.len() < self.cap {
            state.at.push_back(now);
            return Room::Yes;
        }
        let first = state.over == 0;
        state.over = state.over.saturating_add(events);
        Room::No {
            wake_in: first.then_some(window),
        }
    }

    /// How many events the `sync` push to `device` stands for. `None` when
    /// there is nothing to tell.
    pub fn due(&self, device: &Device) -> Option<u32> {
        let mut states = self.states.lock().unwrap();
        let state = states.get_mut(device)?;
        let over = std::mem::take(&mut state.over);
        (over > 0).then_some(over)
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

    const MINUTE: Duration = Duration::from_secs(60);

    fn phone() -> Device {
        ("owner".into(), "phone".into())
    }

    #[test]
    fn a_device_is_pushed_to_as_often_as_it_may_be_and_the_rest_is_counted() {
        let cap = DeviceCap::new(30, MINUTE);
        let start = Instant::now();
        for i in 0..30 {
            assert_eq!(cap.push(&phone(), start + secs(i), 1), Room::Yes, "push {i}");
        }
        assert_eq!(
            cap.push(&phone(), start + secs(30), 1),
            Room::No { wake_in: Some(MINUTE) },
            "the first over the cap sends somebody to come back"
        );
        // A push that stood for five events is five untold.
        assert_eq!(cap.push(&phone(), start + secs(31), 5), Room::No { wake_in: None });
        for i in 0..4 {
            assert_eq!(cap.push(&phone(), start + secs(32 + i), 1), Room::No { wake_in: None });
        }
        assert_eq!(cap.due(&phone()), Some(10), "every one of them is in the count");
        assert_eq!(cap.due(&phone()), None, "told once");
    }

    #[test]
    fn a_push_that_left_the_window_makes_room_for_another() {
        let cap = DeviceCap::new(2, MINUTE);
        let start = Instant::now();
        assert_eq!(cap.push(&phone(), start, 1), Room::Yes);
        assert_eq!(cap.push(&phone(), start + secs(30), 1), Room::Yes);
        assert!(matches!(cap.push(&phone(), start + secs(59), 1), Room::No { .. }));
        // The first of the two is a minute old: room for one, not for two.
        assert_eq!(cap.push(&phone(), start + secs(60), 1), Room::Yes);
        assert!(matches!(cap.push(&phone(), start + secs(61), 1), Room::No { .. }));
    }

    #[test]
    fn what_comes_over_the_cap_is_told_once_in_a_window() {
        let cap = DeviceCap::new(1, MINUTE);
        let start = Instant::now();
        assert_eq!(cap.push(&phone(), start, 1), Room::Yes);
        // Somebody is sent to come back at 61, and what comes until then
        // sends nobody else.
        assert_eq!(cap.push(&phone(), start + secs(1), 1), Room::No { wake_in: Some(MINUTE) });
        assert_eq!(cap.push(&phone(), start + secs(40), 1), Room::No { wake_in: None });
        assert_eq!(cap.due(&phone()), Some(2));

        // The flood goes on: the next to come back does so a minute later.
        assert_eq!(cap.push(&phone(), start + secs(61), 1), Room::Yes);
        assert_eq!(cap.push(&phone(), start + secs(62), 1), Room::No { wake_in: Some(MINUTE) });
    }

    #[test]
    fn devices_do_not_use_up_each_others_pushes() {
        let cap = DeviceCap::new(1, MINUTE);
        let now = Instant::now();
        assert_eq!(cap.push(&phone(), now, 1), Room::Yes);
        assert_eq!(cap.push(&("owner".into(), "tablet".into()), now, 1), Room::Yes);
        assert_eq!(cap.push(&("other".into(), "phone".into()), now, 1), Room::Yes);
        assert_eq!(cap.due(&phone()), None);
    }

    #[test]
    fn a_device_that_has_something_untold_is_kept_and_the_quiet_ones_are_dropped() {
        let cap = DeviceCap::new(1, MINUTE);
        let start = Instant::now();
        for i in 0..DeviceCap::SWEEP_AT {
            cap.push(&("owner".into(), format!("device-{i}")), start, 1);
        }
        let waiting: Device = ("owner".into(), "device-0".into());
        assert!(matches!(cap.push(&waiting, start + secs(1), 1), Room::No { .. }));

        cap.push(&phone(), start + MINUTE + secs(1), 1);
        assert_eq!(cap.states.lock().unwrap().len(), 2, "the one that waits, and the new one");
        assert_eq!(cap.due(&waiting), Some(1));
    }
}
