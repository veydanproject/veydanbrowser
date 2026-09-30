//! What an event is, as far as the server can tell from its outside.
//!
//! The server never opens an event. It reads the kind and the tags, which is
//! what a relay reads too.

use nostr::event::Event;

/// Gift wrap: a sealed direct message (NIP-59).
pub const KIND_WRAP: u16 = 1059;
/// An event of a group.
pub const KIND_GROUP: u16 = 9;

/// What is written on an event for the push server. The messenger's side is
/// `messenger-dm::pushtags`; the two must agree, and a value in the tests of
/// both says that they do.
pub const TAG_SILENT: &str = "silent";
pub const TAG_AUTHOR: &str = "vp";
pub const TAG_GROUP: &str = "gp";

/// How many marks of its group are read off one event. An app puts one, or
/// two for a week after the key of the group changed; whatever comes after
/// the fourth was put there to make the server work.
const GROUP_MARKS: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    /// A direct message to these keys.
    Dm { recipients: Vec<String> },
    Group { id: String },
}

impl Subject {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Dm { .. } => "dm",
            Self::Group { .. } => "group",
        }
    }
}

pub fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn values<'a>(event: &'a Event, name: &'a str) -> impl Iterator<Item = &'a str> {
    event.tags.iter().filter_map(move |t| match t.as_slice() {
        [kind, value, ..] if kind == name => Some(value.as_str()),
        _ => None,
    })
}

/// `None`: not an event the server has anything to do with.
pub fn subject(event: &Event) -> Option<Subject> {
    match event.kind.as_u16() {
        KIND_WRAP => {
            let mut recipients: Vec<String> = values(event, "p")
                .filter(|p| is_hex64(p))
                .map(str::to_string)
                .collect();
            recipients.sort();
            recipients.dedup();
            // One event cannot be allowed to wake a crowd.
            recipients.truncate(8);
            (!recipients.is_empty()).then_some(Subject::Dm { recipients })
        }
        KIND_GROUP => values(event, "h")
            .find(|h| is_hex64(h))
            .map(|id| Subject::Group { id: id.to_string() }),
        _ => None,
    }
}

/// The author marked the event as not worth a push.
pub fn is_silent(event: &Event) -> bool {
    values(event, TAG_SILENT).next().is_some()
}

/// The mark of the author on a group event.
pub fn author_mark(event: &Event) -> Option<&str> {
    values(event, TAG_AUTHOR).next()
}

/// A mark as `mark_of` makes it: 16 hex characters.
fn is_mark(s: &str) -> bool {
    s.len() == 16 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The marks of the group on a group event: what is a mark among the first
/// `GROUP_MARKS` tags `gp`.
pub fn group_marks(event: &Event) -> Vec<&str> {
    values(event, TAG_GROUP)
        .take(GROUP_MARKS)
        .filter(|mark| is_mark(mark))
        .collect()
}

/// Of the push keys of a group, those whose mark is among `marks`, the ones
/// the event carries: a holder of such a key marked the event.
pub fn marking(event: &Event, marks: &[&str], push_keys: Vec<String>) -> Vec<String> {
    let signer = event.pubkey.to_hex();
    push_keys
        .into_iter()
        .filter(|key| mark_of(key, &signer).is_some_and(|mark| marks.contains(&mark.as_str())))
        .collect()
}

/// The mark the holder of a key would have put on an event signed by
/// `event_pubkey`. The key is an author's (`vp`) or a push key of a group
/// (`gp`): both marks are made the same way. See `messenger-dm::pushtags`
/// for what they are for.
pub fn mark_of(key_hex: &str, event_pubkey_hex: &str) -> Option<String> {
    fn bytes(hex: &str) -> Option<Vec<u8>> {
        if !is_hex64(hex) {
            return None;
        }
        (0..32)
            .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok())
            .collect()
    }
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &bytes(key_hex)?);
    let tag = ring::hmac::sign(&key, &bytes(event_pubkey_hex)?);
    Some(tag.as_ref()[..8].iter().map(|b| format!("{b:02x}")).collect())
}

/// Was the event written by the holder of `author_key`.
pub fn written_by(event: &Event, author_key: Option<&str>) -> bool {
    match (author_mark(event), author_key) {
        (Some(mark), Some(key)) => {
            mark_of(key, &event.pubkey.to_hex()).is_some_and(|mine| mine == mark)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::prelude::*;

    fn event(kind: u16, tags: &[&[&str]]) -> Event {
        EventBuilder::new(Kind::from(kind), "sealed")
            .tags(tags.iter().map(|t| Tag::parse(t.iter().copied()).unwrap()))
            .finalize(&Keys::generate())
            .unwrap()
    }

    #[test]
    fn a_wrap_is_for_the_keys_it_names() {
        let (a, b) = ("aa".repeat(32), "bb".repeat(32));
        let e = event(1059, &[&["p", &b], &["p", &a], &["p", &a], &["p", "not-a-key"]]);
        assert_eq!(subject(&e), Some(Subject::Dm { recipients: vec![a, b] }));
        assert_eq!(subject(&event(1059, &[])), None);
    }

    #[test]
    fn one_wrap_cannot_wake_a_crowd() {
        let keys: Vec<String> = (0..50).map(|i| format!("{i:064x}")).collect();
        let tags: Vec<Vec<&str>> = keys.iter().map(|k| vec!["p", k.as_str()]).collect();
        let tags: Vec<&[&str]> = tags.iter().map(|t| t.as_slice()).collect();
        match subject(&event(1059, &tags)) {
            Some(Subject::Dm { recipients }) => assert_eq!(recipients.len(), 8),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_group_event_is_of_the_group_it_names() {
        let g = "11".repeat(32);
        assert_eq!(subject(&event(9, &[&["h", &g], &["k", "key-1"]])), Some(Subject::Group { id: g }));
        assert_eq!(subject(&event(9, &[&["h", "short"]])), None);
        assert_eq!(subject(&event(9, &[])), None);
    }

    #[test]
    fn other_kinds_are_none_of_the_servers_business() {
        let g = "11".repeat(32);
        for kind in [0, 1, 4, 14, 1060, 30078] {
            assert_eq!(subject(&event(kind, &[&["p", &g], &["h", &g]])), None, "{kind}");
        }
    }

    #[test]
    fn silence_is_a_tag_on_the_outside() {
        let a = "aa".repeat(32);
        assert!(is_silent(&event(1059, &[&["p", &a], &["silent", "1"]])));
        assert!(!is_silent(&event(1059, &[&["p", &a]])));
        assert!(!is_silent(&event(1059, &[&["p", "silent"]])));
    }

    /// The same values are in the tests of `messenger-dm::pushtags`. They
    /// were computed apart from both, with openssl.
    #[test]
    fn the_mark_is_made_the_way_the_messenger_makes_it() {
        let key = "48d060b886c7db36283c9e657795adb5f2dc5c7886eadec18610b91b11b9c8e8";
        assert_eq!(mark_of(key, &"02".repeat(32)).as_deref(), Some("20526ea0e3c8f745"));
        assert_eq!(mark_of("short", &"02".repeat(32)), None);
        assert_eq!(mark_of(key, "short"), None);
    }

    /// The mark of a group, from the key of the group to the mark on an
    /// event. The server is given the push key and never the key of the
    /// group; the first step is made here to see the whole value, which is
    /// in the tests of the messenger too and was computed with openssl.
    #[test]
    fn the_mark_of_a_group_is_made_the_way_the_messenger_makes_it() {
        let group_key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &[0x03; 32]);
        let push_key: String = ring::hmac::sign(&group_key, b"veydan-push-group-v1")
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(push_key, "c960ddf616adb9a57ac7d0eb04da5e25cf3d139c95d5828addb8f1f0ed50da28");
        assert_eq!(mark_of(&push_key, &"02".repeat(32)).as_deref(), Some("86a8b9acabf6941d"));
    }

    /// A group event signed by `signer`, with these tags after its `h`.
    fn group_event(signer: &Keys, tags: &[&[&str]]) -> Event {
        let mut all = vec![Tag::parse(["h", &"11".repeat(32)]).unwrap()];
        all.extend(tags.iter().map(|t| Tag::parse(t.iter().copied()).unwrap()));
        EventBuilder::new(Kind::from(9u16), "sealed").tags(all).finalize(signer).unwrap()
    }

    #[test]
    fn the_marks_of_a_group_are_sixteen_hex_characters_in_the_first_four_tags() {
        let signer = Keys::generate();
        let (a, b) = ("0123456789abcdef", "fedcba9876543210");
        assert!(group_marks(&group_event(&signer, &[])).is_empty());
        assert_eq!(group_marks(&group_event(&signer, &[&["gp", a]])), [a]);
        // After a change of the key: the mark of the key of now, and of the
        // one before.
        assert_eq!(group_marks(&group_event(&signer, &[&["gp", a], &["vp", b], &["gp", b]])), [a, b]);

        for not_a_mark in ["", "0123456789abcde", "0123456789abcdef0", "0123456789ABCDEF", "0123456789abcdeg"] {
            let e = group_event(&signer, &[&["gp", not_a_mark], &["gp", a]]);
            assert_eq!(group_marks(&e), [a], "{not_a_mark:?} is passed over");
        }
        // A tag with nothing in it is no `gp` tag at all.
        assert_eq!(group_marks(&group_event(&signer, &[&["gp"], &["gp", a]])), [a]);

        let fifth = group_event(&signer, &[&["gp", "x"], &["gp", "x"], &["gp", "x"], &["gp", "x"], &["gp", a]]);
        assert!(group_marks(&fifth).is_empty(), "the fifth is not read");
        let fourth = group_event(&signer, &[&["gp", "x"], &["gp", "x"], &["gp", "x"], &["gp", a], &["gp", b]]);
        assert_eq!(group_marks(&fourth), [a]);
    }

    #[test]
    fn a_holder_of_the_push_key_is_told_from_a_stranger() {
        let (key, other) = ("c1".repeat(32), "c2".repeat(32));
        let signer = Keys::generate();
        let mark = mark_of(&key, &signer.public_key().to_hex()).unwrap();
        let e = group_event(&signer, &[&["gp", mark.as_str()]]);
        let keys = vec![other, "not a key".to_string(), key.clone()];

        assert_eq!(marking(&e, &group_marks(&e), keys.clone()), [key]);
        // The mark is of the key that signed the event: copied onto an
        // event of another signer, it is nobody's.
        let copied = group_event(&Keys::generate(), &[&["gp", mark.as_str()]]);
        assert!(marking(&copied, &group_marks(&copied), keys).is_empty());
    }

    #[test]
    fn the_author_is_told_from_the_others() {
        let (mine, other) = ("11".repeat(32), "22".repeat(32));
        let signer = Keys::generate();
        let mark = mark_of(&mine, &signer.public_key().to_hex()).unwrap();
        let e = EventBuilder::new(Kind::from(9u16), "sealed")
            .tags([
                Tag::parse(["h", &"11".repeat(32)]).unwrap(),
                Tag::parse(["vp", mark.as_str()]).unwrap(),
            ])
            .finalize(&signer)
            .unwrap();

        assert!(written_by(&e, Some(&mine)));
        assert!(!written_by(&e, Some(&other)));
        assert!(!written_by(&e, None), "a device that gave no key is nobody's author");
        assert!(!written_by(&event(9, &[&["h", &mine]]), Some(&mine)), "an event without a mark");
    }
}
