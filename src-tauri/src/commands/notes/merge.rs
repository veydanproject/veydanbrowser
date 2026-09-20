// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! 3-way merge shared by history merge and vault sync. Line endings are
//! normalized to LF first so CRLF/LF differences never count as changes.
//! Conflicts are returned as structured blocks; the UI never parses markers.

use diffy::{ConflictStyle, MergeOptions};
use serde::Serialize;

/// Long markers so real text lines like `=======` cannot be mistaken for one.
const MARKER_LEN: usize = 32;

#[derive(Debug, Serialize, Clone)]
pub struct MergeBlock {
    /// "normal" | "conflict"
    pub kind: String,
    /// Text of a normal block
    pub text: String,
    /// Conflict sides
    pub ours: String,
    pub theirs: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct MergeResult {
    /// Clean merged text; with conflicts it still carries markers, use `blocks` instead
    pub content: String,
    pub has_conflicts: bool,
    pub blocks: Vec<MergeBlock>,
}

pub fn normalize_eol(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

pub fn merge3(ancestor: &str, ours: &str, theirs: &str) -> MergeResult {
    let (a, o, t) = (normalize_eol(ancestor), normalize_eol(ours), normalize_eol(theirs));
    let mut opts = MergeOptions::new();
    opts.set_conflict_style(ConflictStyle::Merge).set_conflict_marker_length(MARKER_LEN);
    match opts.merge(&a, &o, &t) {
        Ok(content) => MergeResult {
            blocks: vec![MergeBlock { kind: "normal".into(), text: content.clone(), ours: String::new(), theirs: String::new() }],
            content,
            has_conflicts: false,
        },
        Err(content) => MergeResult { blocks: parse_blocks(&content), content, has_conflicts: true },
    }
}

fn marker(ch: char) -> String {
    std::iter::repeat(ch).take(MARKER_LEN).collect()
}

/// Split diffy's marked output into normal and conflict blocks.
/// Block texts keep their trailing newline so joining them restores the file.
fn parse_blocks(marked: &str) -> Vec<MergeBlock> {
    let (start, sep, end) = (marker('<'), marker('='), marker('>'));
    let mut blocks = Vec::new();
    let mut normal = String::new();
    let mut ours: Option<String> = None;
    let mut theirs: Option<String> = None;

    for line in marked.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.starts_with(&start) {
            if !normal.is_empty() {
                blocks.push(MergeBlock { kind: "normal".into(), text: std::mem::take(&mut normal), ours: String::new(), theirs: String::new() });
            }
            ours = Some(String::new());
            continue;
        }
        if trimmed == sep && ours.is_some() && theirs.is_none() {
            theirs = Some(String::new());
            continue;
        }
        if trimmed.starts_with(&end) && theirs.is_some() {
            blocks.push(MergeBlock {
                kind: "conflict".into(),
                text: String::new(),
                ours: ours.take().unwrap_or_default(),
                theirs: theirs.take().unwrap_or_default(),
            });
            continue;
        }
        match (&mut ours, &mut theirs) {
            (_, Some(t)) => t.push_str(line),
            (Some(o), None) => o.push_str(line),
            _ => normal.push_str(line),
        }
    }
    if !normal.is_empty() {
        blocks.push(MergeBlock { kind: "normal".into(), text: normal, ours: String::new(), theirs: String::new() });
    }
    blocks
}
