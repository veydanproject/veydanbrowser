//! What the server has counted since it started, for `vpush ctl stats`.
//!
//! These say how often the server had to defend itself. They are kept in
//! memory and begin at zero with every start: a number that grows between
//! two looks is what is happening now.

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Serialize, Serializer};

#[derive(Debug, Default)]
pub struct Counter(AtomicU64);

impl Counter {
    pub fn add(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }

    pub fn get(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

impl Serialize for Counter {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(self.get())
    }
}

#[derive(Debug, Default, Serialize)]
pub struct Counters {
    /// Registrations refused because their owner registers too often.
    pub refused_rate_owner: Counter,
    /// Registrations of a new device refused because too many came from
    /// one address.
    pub refused_rate_ip: Counter,
    /// Registrations refused because the server holds as many devices as
    /// it takes.
    pub refused_devices_total: Counter,
    /// Registrations refused because the push service does not know the token.
    pub tokens_invalid: Counter,
    /// Pushes that said how many events came over what a device is pushed
    /// about one by one.
    pub sync_pushes: Counter,
    /// Times a relay's line had more to deal with than it has room for,
    /// and started over.
    pub lines_started_over: Counter,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_are_written_as_numbers() {
        let counters = Counters::default();
        counters.tokens_invalid.add();
        counters.tokens_invalid.add();
        counters.sync_pushes.add();
        assert_eq!(
            serde_json::to_value(&counters).unwrap(),
            serde_json::json!({
                "refused_rate_owner": 0,
                "refused_rate_ip": 0,
                "refused_devices_total": 0,
                "tokens_invalid": 2,
                "sync_pushes": 1,
                "lines_started_over": 0,
            })
        );
    }
}
