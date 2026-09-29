//! Attempts after the first one.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::{Attempt, Message, Outcome, PushProvider, Target};

#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    /// Wait before the second attempt; doubles with each one after.
    pub base: Duration,
    /// No single wait is longer. A service that asks for more is not waited
    /// for here: the push is given back as not delivered.
    pub max_wait: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base: Duration::from_secs(1),
            max_wait: Duration::from_secs(30),
        }
    }
}

impl RetryPolicy {
    /// How long to wait after attempt number `done` (from 1), or `None` to stop.
    fn wait(&self, done: u32, asked: Option<Duration>) -> Option<Duration> {
        if done >= self.max_attempts {
            return None;
        }
        let own = self
            .base
            .saturating_mul(2u32.saturating_pow(done.saturating_sub(1)))
            .min(self.max_wait);
        match asked {
            Some(asked) if asked > self.max_wait => None,
            Some(asked) => Some(asked.max(own)),
            None => Some(own),
        }
    }
}

/// Every attempt made for one push; the last one is the result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delivery {
    pub outcome: Outcome,
    pub attempts: Vec<Attempt>,
}

pub async fn deliver(
    provider: &dyn PushProvider,
    target: &Target,
    message: &Message,
    policy: RetryPolicy,
) -> Delivery {
    let mut attempts = Vec::new();
    loop {
        let attempt = provider.send(target, message).await;
        let n = attempts.len() as u32 + 1;
        tracing::debug!(
            provider = provider.kind().as_str(),
            token = %target.masked(),
            trace = message.payload.trace.as_deref().unwrap_or(""),
            attempt = n,
            outcome = ?attempt.outcome,
            http_status = attempt.http_status,
            code = attempt.code.as_deref(),
            detail = attempt.detail.as_deref(),
            latency_ms = attempt.latency_ms,
            "push attempt"
        );
        let outcome = attempt.outcome;
        let asked = attempt.retry_after_ms.map(Duration::from_millis);
        attempts.push(attempt);

        if outcome != Outcome::Retry {
            return Delivery { outcome, attempts };
        }
        match policy.wait(n, asked) {
            Some(wait) => tokio::time::sleep(wait).await,
            None => return Delivery { outcome, attempts },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> RetryPolicy {
        RetryPolicy {
            max_attempts: 4,
            base: Duration::from_secs(1),
            max_wait: Duration::from_secs(5),
        }
    }

    #[test]
    fn waits_double_and_stop_at_the_cap() {
        let p = policy();
        assert_eq!(p.wait(1, None), Some(Duration::from_secs(1)));
        assert_eq!(p.wait(2, None), Some(Duration::from_secs(2)));
        assert_eq!(p.wait(3, None), Some(Duration::from_secs(4)));
        assert_eq!(p.wait(4, None), None, "attempts are used up");
    }

    #[test]
    fn the_service_is_obeyed_when_it_asks_for_more() {
        let p = policy();
        assert_eq!(
            p.wait(1, Some(Duration::from_secs(3))),
            Some(Duration::from_secs(3))
        );
    }

    #[test]
    fn the_service_cannot_ask_for_less_than_our_own_wait() {
        let p = policy();
        assert_eq!(
            p.wait(3, Some(Duration::from_secs(1))),
            Some(Duration::from_secs(4))
        );
    }

    #[test]
    fn a_wait_beyond_the_cap_ends_the_attempts() {
        let p = policy();
        assert_eq!(p.wait(1, Some(Duration::from_secs(600))), None);
    }
}
