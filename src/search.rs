//! Buffer search (ticket #8, phase 1): one regex dialect (the Rust `regex`
//! crate), smartcase like the author's nvim, matches cached per buffer
//! version for hlsearch rendering and n/N navigation.

use regex::Regex;
use ropey::Rope;

/// Matches over 2MB of text render plain and search silently degrades.
const MAX_SEARCH_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    #[default]
    Forward,
    Backward,
}

/// One match: line + char columns (start, end-exclusive).
pub type Match = (usize, u32, u32);

#[derive(Default)]
pub struct Search {
    pub pattern: String,
    pub direction: Direction,
    regex: Option<Regex>,
    /// All matches in buffer order, recomputed when the buffer changes.
    matches: Vec<Match>,
    version: Option<(usize, u64)>,
    /// Highlighting enabled (`:noh` / Esc clears until the next search).
    pub highlight: bool,
}

/// smartcase: insensitive unless the pattern contains an uppercase letter.
pub fn compile(pattern: &str) -> Option<Regex> {
    if pattern.is_empty() {
        return None;
    }
    let sensitive = pattern.chars().any(|c| c.is_uppercase());
    let src = if sensitive {
        pattern.to_string()
    } else {
        format!("(?i){pattern}")
    };
    Regex::new(&src).ok()
}

impl Search {
    /// Sets a new pattern (compiling it) and enables highlighting.
    /// Returns false when the pattern doesn't compile.
    pub fn set_pattern(&mut self, pattern: &str, direction: Direction) -> bool {
        self.pattern = pattern.to_string();
        self.direction = direction;
        self.regex = compile(pattern);
        self.version = None; // force recompute
        self.highlight = true;
        self.regex.is_some()
    }

    pub fn is_active(&self) -> bool {
        self.regex.is_some()
    }

    /// Recomputes matches if the buffer (id or version) changed.
    pub fn ensure(&mut self, rope: &Rope, buf: usize, version: u64) {
        if self.version == Some((buf, version)) {
            return;
        }
        self.version = Some((buf, version));
        self.matches.clear();
        let Some(re) = &self.regex else { return };
        if rope.len_bytes() > MAX_SEARCH_BYTES {
            return;
        }
        let text = rope.to_string();
        for (line_idx, line) in text.split('\n').enumerate() {
            for m in re.find_iter(line) {
                if m.start() == m.end() {
                    continue; // zero-width matches would loop the cursor
                }
                let start = line[..m.start()].chars().count() as u32;
                let end = start + line[m.start()..m.end()].chars().count() as u32;
                self.matches.push((line_idx, start, end));
            }
        }
    }

    pub fn matches_on_line(&self, line: usize) -> impl Iterator<Item = (u32, u32)> + '_ {
        self.matches
            .iter()
            .filter(move |(l, ..)| *l == line)
            .map(|(_, s, e)| (*s, *e))
    }

    pub fn count(&self) -> usize {
        self.matches.len()
    }

    /// First match strictly after (line, col), wrapping. Also reports the wrap.
    pub fn next_after(&self, line: usize, col: u32) -> Option<(Match, bool)> {
        if self.matches.is_empty() {
            return None;
        }
        let idx = self
            .matches
            .iter()
            .position(|(l, s, _)| (*l, *s) > (line, col));
        match idx {
            Some(i) => Some((self.matches[i], false)),
            None => Some((self.matches[0], true)),
        }
    }

    /// Last match strictly before (line, col), wrapping backward.
    pub fn prev_before(&self, line: usize, col: u32) -> Option<(Match, bool)> {
        if self.matches.is_empty() {
            return None;
        }
        let idx = self
            .matches
            .iter()
            .rposition(|(l, s, _)| (*l, *s) < (line, col));
        match idx {
            Some(i) => Some((self.matches[i], false)),
            None => Some((*self.matches.last().unwrap(), true)),
        }
    }

    /// Match at-or-after (for landing on a match under the cursor: `*`).
    pub fn next_from(&self, line: usize, col: u32) -> Option<(Match, bool)> {
        if self.matches.is_empty() {
            return None;
        }
        let idx = self
            .matches
            .iter()
            .position(|(l, s, _)| (*l, *s) >= (line, col));
        match idx {
            Some(i) => Some((self.matches[i], false)),
            None => Some((self.matches[0], true)),
        }
    }

    /// The match containing or starting at the cursor, for current-match
    /// highlighting.
    pub fn match_at(&self, line: usize, col: u32) -> Option<(u32, u32)> {
        self.matches
            .iter()
            .find(|(l, s, e)| *l == line && *s <= col && col < *e)
            .map(|(_, s, e)| (*s, *e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn searched(pattern: &str, text: &str) -> Search {
        let mut s = Search::default();
        assert!(s.set_pattern(pattern, Direction::Forward));
        s.ensure(&Rope::from_str(text), 1, 1);
        s
    }

    #[test]
    fn smartcase() {
        let s = searched("hello", "Hello hello HELLO\n");
        assert_eq!(s.count(), 3);
        let s = searched("Hello", "Hello hello HELLO\n");
        assert_eq!(s.count(), 1);
    }

    #[test]
    fn navigation_wraps_both_ways() {
        let s = searched("x", "x.x\n.x.\n");
        assert_eq!(s.count(), 3);
        assert_eq!(s.next_after(0, 0), Some(((0, 2, 3), false)));
        assert_eq!(s.next_after(1, 1), Some(((0, 0, 1), true)));
        assert_eq!(s.prev_before(0, 0), Some(((1, 1, 2), true)));
        assert_eq!(s.prev_before(1, 1), Some(((0, 2, 3), false)));
    }

    #[test]
    fn unicode_columns() {
        let s = searched("x", "日本x語x\n");
        assert_eq!(s.next_from(0, 0), Some(((0, 2, 3), false)));
        assert_eq!(s.next_after(0, 2), Some(((0, 4, 5), false)));
    }

    #[test]
    fn invalid_pattern_is_inert() {
        let mut s = Search::default();
        assert!(!s.set_pattern("(unclosed", Direction::Forward));
        s.ensure(&Rope::from_str("(unclosed\n"), 1, 1);
        assert_eq!(s.count(), 0);
    }

    #[test]
    fn zero_width_matches_are_skipped() {
        let s = searched("a*", "bbb\n");
        assert_eq!(s.count(), 0);
    }
}
