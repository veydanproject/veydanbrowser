// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! NIP-17 wrapping. One rumor, two gift wraps: one the peer can open, one
//! we can open (self-copy: history on our other devices and after a
//! relogin). Both carry the same rumor, so the rumor id identifies the
//! message everywhere.

use messenger_core::outbound::WireEvent;
use messenger_core::{EventId, MessengerError, PubKey, Result};
use nostr::key::Keys;
use nostr::nips::nip59::GiftWrapBuilder;
use nostr::prelude::*;

pub struct Wrapped {
    pub rumor_id: EventId,
    pub to_peer: WireEvent,
    /// `None` when the peer is ourselves (notes to self need one copy).
    pub to_self: Option<WireEvent>,
}

fn crypto(e: impl std::fmt::Display) -> MessengerError {
    MessengerError::Crypto(e.to_string())
}

pub fn wrap(keys: &Keys, peer: &PubKey, content: &str, created_at: i64, reply_to: Option<&str>) -> Result<Wrapped> {
    let receiver = PublicKey::from_hex(peer.as_hex()).map_err(crypto)?;
    let me = keys.public_key();
    let mut builder = EventBuilder::new(Kind::PrivateDirectMessage, content)
        .tag(Tag::public_key(receiver))
        .custom_created_at(nostr::types::Timestamp::from_secs(created_at.max(0) as u64));
    if let Some(id) = reply_to {
        builder = builder.tag(Tag::parse(["e", id]).map_err(crypto)?);
    }
    let mut rumor = builder.finalize_unsigned(me);
    rumor.ensure_id();
    let rumor_id = rumor
        .id
        .and_then(|id| EventId::parse(&id.to_hex()))
        .ok_or_else(|| MessengerError::Crypto("rumor has no id".into()))?;

    let to_peer = wire(GiftWrapBuilder::new(receiver, rumor.clone()).finalize(keys).map_err(crypto)?)?;
    let to_self = if receiver == me {
        None
    } else {
        Some(wire(GiftWrapBuilder::new(me, rumor).finalize(keys).map_err(crypto)?)?)
    };
    Ok(Wrapped { rumor_id, to_peer, to_self })
}

fn wire(event: Event) -> Result<WireEvent> {
    Ok(WireEvent {
        id: EventId::parse(&event.id.to_hex()).ok_or_else(|| MessengerError::Crypto("bad event id".into()))?,
        json: serde_json::to_value(&event)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::nips::nip59::UnwrappedGift;

    fn pk(k: &Keys) -> PubKey {
        PubKey::parse(&k.public_key().to_hex()).unwrap()
    }

    #[test]
    fn both_copies_carry_the_same_rumor() {
        let alice = Keys::generate();
        let bob = Keys::generate();
        let w = wrap(&alice, &pk(&bob), "hi", 1_700_000_000, Some(&"ab".repeat(32))).unwrap();
        let peer_ev: Event = serde_json::from_value(w.to_peer.json.clone()).unwrap();
        let self_ev: Event = serde_json::from_value(w.to_self.as_ref().unwrap().json.clone()).unwrap();
        assert_ne!(peer_ev.id, self_ev.id, "two different wraps");

        let by_bob = UnwrappedGift::from_gift_wrap(&bob, &peer_ev).unwrap();
        let by_alice = UnwrappedGift::from_gift_wrap(&alice, &self_ev).unwrap();
        assert!(UnwrappedGift::from_gift_wrap(&bob, &self_ev).is_err(), "bob cannot open alice's copy");
        for u in [&by_bob, &by_alice] {
            let mut r = u.rumor.clone();
            r.ensure_id();
            assert_eq!(r.id.unwrap().to_hex(), w.rumor_id.as_hex());
            assert_eq!(r.created_at.as_secs(), 1_700_000_000);
            assert_eq!(r.content, "hi");
            assert_eq!(u.sender, alice.public_key());
            assert!(r.tags.iter().any(|t| t.kind() == "e"));
        }
    }

    #[test]
    fn note_to_self_has_one_copy() {
        let me = Keys::generate();
        let w = wrap(&me, &pk(&me), "memo", 1, None).unwrap();
        assert!(w.to_self.is_none());
    }
}
