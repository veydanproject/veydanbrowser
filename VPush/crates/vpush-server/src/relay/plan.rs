//! What to ask of a relay.

use crate::store::{RelayPlan, WatchKind};

/// Keys or groups in one subscription. Relays refuse a request that is too
/// long; this many fit.
pub const CHUNK: usize = 500;

/// A sealed direct message is dated up to two days back by its sender, to
/// hide when it was written. So the newest message may carry a date of two
/// days ago, and that far the relay has to be asked.
pub const WRAP_WINDOW: u64 = 2 * 86_400 + 600;

/// Group events are dated when they are written. This much is asked for
/// again, for the clocks that differ.
pub const GROUP_BACK: u64 = 600;

/// One request to a relay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sub {
    /// `dm-0`, `group-3`
    pub id: String,
    pub kind: WatchKind,
    pub targets: Vec<String>,
    /// Unix seconds.
    pub since: u64,
}

/// The requests for a plan. `last_alive` is when the server last had the
/// relay on the line: group events are asked for from a little before that,
/// so that what came while the server was away is not lost.
pub fn subscriptions(plan: &RelayPlan, now: u64, last_alive: Option<u64>) -> Vec<Sub> {
    let group_since = last_alive
        .unwrap_or(now)
        .min(now)
        .saturating_sub(GROUP_BACK)
        .max(now.saturating_sub(WRAP_WINDOW));

    let chunks = |kind: WatchKind, targets: &[String], since: u64| -> Vec<Sub> {
        targets
            .chunks(CHUNK)
            .enumerate()
            .map(|(i, chunk)| Sub {
                id: format!("{}-{i}", kind.as_str()),
                kind,
                targets: chunk.to_vec(),
                since,
            })
            .collect()
    };
    let mut out = chunks(WatchKind::Dm, &plan.dm, now.saturating_sub(WRAP_WINDOW));
    out.extend(chunks(WatchKind::Group, &plan.groups, group_since));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_790_000_000;

    fn plan(dm: usize, groups: usize) -> RelayPlan {
        RelayPlan {
            url: "wss://relay.example.org".into(),
            dm: (0..dm).map(|i| format!("{i:064x}")).collect(),
            groups: (0..groups).map(|i| format!("{:064x}", i + 1_000_000)).collect(),
        }
    }

    #[test]
    fn direct_messages_are_asked_for_two_days_back() {
        let subs = subscriptions(&plan(1, 0), NOW, Some(NOW));
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].id, "dm-0");
        assert_eq!(subs[0].since, NOW - 2 * 86_400 - 600);
    }

    #[test]
    fn group_events_are_asked_for_from_when_the_relay_was_last_on_the_line() {
        let since = |last_alive| subscriptions(&plan(0, 1), NOW, last_alive)[0].since;
        assert_eq!(since(None), NOW - 600, "never seen before: from now");
        assert_eq!(since(Some(NOW - 100)), NOW - 700);
        assert_eq!(since(Some(NOW - 3600)), NOW - 4200, "an hour away");
        assert_eq!(since(Some(NOW - 30 * 86_400)), NOW - WRAP_WINDOW, "no further than two days");
        assert_eq!(since(Some(NOW + 500)), NOW - 600, "a clock that ran ahead");
    }

    #[test]
    fn long_lists_are_cut_so_that_a_relay_takes_them() {
        let subs = subscriptions(&plan(1201, 501), NOW, None);
        let sizes: Vec<_> = subs.iter().map(|s| (s.id.as_str(), s.targets.len())).collect();
        assert_eq!(
            sizes,
            [("dm-0", 500), ("dm-1", 500), ("dm-2", 201), ("group-0", 500), ("group-1", 1)]
        );
        let all: usize = subs.iter().filter(|s| s.kind == WatchKind::Dm).map(|s| s.targets.len()).sum();
        assert_eq!(all, 1201);
    }

    #[test]
    fn nothing_to_watch_is_nothing_to_ask() {
        assert!(subscriptions(&plan(0, 0), NOW, None).is_empty());
    }
}
