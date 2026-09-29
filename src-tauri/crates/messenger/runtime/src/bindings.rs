// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! TypeScript types of what the runtime hands to the UI, written from the
//! Rust types themselves so the two cannot drift apart.
//!
//! The file lives with the UI (`src/lib/messenger/generated/`). A test
//! compares it with what the types say now and fails when it is stale;
//! `make msg-types` (or `UPDATE_TS_BINDINGS=1 cargo test -p
//! messenger-runtime bindings`) writes it again.

use crate::links::{GroupMembership, LinkGroupKind, LinkView};
use messenger_preview::Preview;
use ts_rs::{Config, TS};

/// Relative to this crate.
pub const LINKS_FILE: &str = "../../../../src/lib/messenger/generated/links.ts";

const HEADER: &str = "// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

// Generated from Rust (messenger-runtime/src/bindings.rs). Do not edit:
// run `make msg-types` after changing the Rust types.
";

fn export<T: TS>(cfg: &Config, doc: &str) -> String {
    format!("\n/** {doc} */\nexport {}\n", T::decl(cfg))
}

/// The whole file, as the types say it should be.
pub fn links_ts() -> String {
    let cfg = Config::new();
    let mut out = String::from(HEADER);
    out += &export::<GroupMembership>(&cfg, "What I am to a group, as far as this device knows.");
    out += &export::<LinkGroupKind>(&cfg, "Whether a group lets anyone in by its link.");
    out += &export::<LinkView>(&cfg, "What a link inside leads to, as the runtime sees it. The UI never takes a link apart itself.");
    out += &export::<Preview>(&cfg, "What a page outside says about itself.");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn bindings_are_current() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(LINKS_FILE);
        let want = links_ts();
        if std::env::var_os("UPDATE_TS_BINDINGS").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &want).unwrap();
            return;
        }
        let have = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(have == want, "{} is stale: run `make msg-types`", path.display());
    }

    #[test]
    fn every_membership_the_store_writes_has_a_name_in_typescript() {
        use messenger_groups::service::*;
        for word in [
            MEMBERSHIP_JOINED, MEMBERSHIP_JOINING, MEMBERSHIP_REQUESTED, MEMBERSHIP_REJECTED, MEMBERSHIP_STALE,
            MEMBERSHIP_LEFT, MEMBERSHIP_REMOVED, MEMBERSHIP_BANNED, MEMBERSHIP_DISBANDED,
        ] {
            let m = GroupMembership::parse(word).unwrap_or_else(|| panic!("{word} is not known"));
            assert_eq!(serde_json::to_value(m).unwrap(), serde_json::json!(word), "the UI gets the same word");
        }
        assert!(GroupMembership::parse("member").is_none());
    }
}
