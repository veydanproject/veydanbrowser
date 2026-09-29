// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

/// What a link leads to. A well-formed link of a type this version does
/// not know is still a link: a newer client wrote it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum LinkType {
    Group,
    Contact,
    Unknown(String),
}

impl LinkType {
    /// `word` is already known to be small latin letters.
    pub(crate) fn of(word: &str) -> Self {
        match word {
            "group" => Self::Group,
            "contact" => Self::Contact,
            other => Self::Unknown(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Group => "group",
            Self::Contact => "contact",
            Self::Unknown(w) => w,
        }
    }
}
