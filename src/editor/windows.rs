//! The split-window tree: layout, focus geometry, and structural operations.
//!
//! Windows form a tree of splits (same-direction splits flatten into one
//! container, like vim's frames). Each leaf carries a window id; the focused
//! window's view state lives checked out in the `Editor` fields, parked
//! windows keep theirs in the leaf. Child sizes are stored in cells along the
//! container's axis and re-normalized proportionally when the terminal size
//! changes.

use ratatui::layout::Rect;

use crate::core::buffer::Cursor;
use crate::core::commands::WinDir;

use super::BufId;

pub type WinId = usize;

/// Minimum window sizes (including the per-window statusline row).
pub const MIN_WIN_HEIGHT: u16 = 2;
pub const MIN_WIN_WIDTH: u16 = 12;

/// View state of a parked (unfocused) window. The focused window's state is
/// checked out into the `Editor` fields and its leaf holds `None`.
#[derive(Debug, Clone, Default)]
pub struct WinState {
    pub buf_id: BufId,
    pub cursor: Cursor,
    pub goal: Option<usize>,
    pub top_line: usize,
    pub alternate: Option<BufId>,
}

pub use crate::core::commands::SplitDir;

pub enum Node {
    Leaf {
        id: WinId,
        /// `None` while this window is focused (checked out).
        state: Option<WinState>,
    },
    Split {
        dir: SplitDir,
        children: Vec<Node>,
        /// Size of each child in cells along the container axis.
        sizes: Vec<u16>,
    },
}

impl Node {
    pub fn leaf(id: WinId, state: Option<WinState>) -> Self {
        Node::Leaf { id, state }
    }

    pub fn window_count(&self) -> usize {
        match self {
            Node::Leaf { .. } => 1,
            Node::Split { children, .. } => children.iter().map(Node::window_count).sum(),
        }
    }

    pub fn window_ids(&self) -> Vec<WinId> {
        match self {
            Node::Leaf { id, .. } => vec![*id],
            Node::Split { children, .. } => children.iter().flat_map(Node::window_ids).collect(),
        }
    }

    pub fn contains(&self, win: WinId) -> bool {
        match self {
            Node::Leaf { id, .. } => *id == win,
            Node::Split { children, .. } => children.iter().any(|c| c.contains(win)),
        }
    }

    pub fn state_mut(&mut self, win: WinId) -> Option<&mut Option<WinState>> {
        match self {
            Node::Leaf { id, state } if *id == win => Some(state),
            Node::Leaf { .. } => None,
            Node::Split { children, .. } => children.iter_mut().find_map(|c| c.state_mut(win)),
        }
    }

    pub fn states_mut<'a>(&'a mut self, out: &mut Vec<(WinId, &'a mut WinState)>) {
        match self {
            Node::Leaf { id, state } => {
                if let Some(s) = state {
                    out.push((*id, s));
                }
            }
            Node::Split { children, .. } => {
                for c in children {
                    c.states_mut(out);
                }
            }
        }
    }

    /// Computes each window's screen rectangle (statusline row included).
    pub fn layout(&self, area: Rect, out: &mut Vec<(WinId, Rect)>) {
        match self {
            Node::Leaf { id, .. } => out.push((*id, area)),
            Node::Split {
                dir,
                children,
                sizes,
            } => {
                let sep = match dir {
                    SplitDir::Vertical => (children.len() - 1) as u16,
                    SplitDir::Horizontal => 0,
                };
                let total = match dir {
                    SplitDir::Vertical => area.width.saturating_sub(sep),
                    SplitDir::Horizontal => area.height,
                };
                let shares = normalized(sizes, total);
                let mut pos = 0;
                for (i, child) in children.iter().enumerate() {
                    let len = shares[i];
                    let rect = match dir {
                        SplitDir::Vertical => Rect::new(
                            area.x + pos + i as u16, // i separators before child i
                            area.y,
                            len,
                            area.height,
                        ),
                        SplitDir::Horizontal => Rect::new(area.x, area.y + pos, area.width, len),
                    };
                    child.layout(rect, out);
                    pos += len;
                }
            }
        }
    }

    /// X positions of the separator columns of vertical containers.
    pub fn separators(&self, area: Rect, out: &mut Vec<Rect>) {
        if let Node::Split {
            dir,
            children,
            sizes,
        } = self
        {
            let sep = match dir {
                SplitDir::Vertical => (children.len() - 1) as u16,
                SplitDir::Horizontal => 0,
            };
            let total = match dir {
                SplitDir::Vertical => area.width.saturating_sub(sep),
                SplitDir::Horizontal => area.height,
            };
            let shares = normalized(sizes, total);
            let mut pos = 0;
            for (i, child) in children.iter().enumerate() {
                let len = shares[i];
                let rect = match dir {
                    SplitDir::Vertical => {
                        Rect::new(area.x + pos + i as u16, area.y, len, area.height)
                    }
                    SplitDir::Horizontal => Rect::new(area.x, area.y + pos, area.width, len),
                };
                if *dir == SplitDir::Vertical && i > 0 {
                    out.push(Rect::new(rect.x - 1, area.y, 1, area.height));
                }
                child.separators(rect, out);
                pos += len;
            }
        }
    }

    /// Splits `win` in `dir`, adding a leaf for `new` after it (below/right).
    /// Same-direction parents gain a sibling instead of nesting (vim frames).
    /// The new window gets half of the split window's share.
    pub fn split(&mut self, win: WinId, dir: SplitDir, new: Node) -> bool {
        // a lone leaf at the root becomes a split
        if let Node::Leaf { id, .. } = self {
            if *id != win {
                return false;
            }
            let old = std::mem::replace(
                self,
                Node::Split {
                    dir,
                    children: Vec::new(),
                    sizes: Vec::new(),
                },
            );
            if let Node::Split {
                children, sizes, ..
            } = self
            {
                children.push(old);
                children.push(new);
                sizes.extend([1, 1]);
            }
            return true;
        }
        self.split_inner(win, dir, new)
    }

    fn split_inner(&mut self, win: WinId, dir: SplitDir, new: Node) -> bool {
        let Node::Split {
            dir: my_dir,
            children,
            sizes,
        } = self
        else {
            return false;
        };
        for (i, child) in children.iter_mut().enumerate() {
            match child {
                Node::Leaf { id, .. } if *id == win => {
                    if *my_dir == dir {
                        // flatten: insert as a sibling, halving the share
                        let half = (sizes[i] / 2).max(1);
                        sizes[i] = (sizes[i] - half).max(1);
                        children.insert(i + 1, new);
                        sizes.insert(i + 1, half);
                    } else {
                        // nest a perpendicular split in place of the leaf
                        let old = std::mem::replace(
                            child,
                            Node::Split {
                                dir,
                                children: Vec::new(),
                                sizes: Vec::new(),
                            },
                        );
                        if let Node::Split {
                            children: sub,
                            sizes: sub_sizes,
                            ..
                        } = child
                        {
                            sub.push(old);
                            sub.push(new);
                            sub_sizes.extend([1, 1]);
                        }
                    }
                    return true;
                }
                _ => {
                    if child.contains(win) {
                        return child.split_inner(win, dir, new);
                    }
                }
            }
        }
        false
    }

    /// Removes `win`, giving its share to the previous sibling (or next when
    /// it was first) and collapsing single-child containers.
    pub fn close(&mut self, win: WinId) -> bool {
        let Node::Split {
            children, sizes, ..
        } = self
        else {
            return false;
        };
        if let Some(i) = children
            .iter()
            .position(|c| matches!(c, Node::Leaf { id, .. } if *id == win))
        {
            let freed = sizes.remove(i);
            children.remove(i);
            if !sizes.is_empty() {
                let neighbor = if i > 0 { i - 1 } else { 0 };
                sizes[neighbor] = sizes[neighbor].saturating_add(freed);
            }
            self.collapse();
            return true;
        }
        for child in children.iter_mut() {
            if child.contains(win) && child.close(win) {
                self.collapse();
                return true;
            }
        }
        false
    }

    /// Replaces a single-child split with its child; merges same-direction
    /// grandchildren into this container.
    fn collapse(&mut self) {
        let Node::Split {
            dir,
            children,
            sizes,
        } = self
        else {
            return;
        };
        if children.len() == 1 {
            *self = children.remove(0);
            return;
        }
        // merge same-direction child containers (keeps frames flat)
        let my_dir = *dir;
        let mut i = 0;
        while i < children.len() {
            let merge = matches!(&children[i], Node::Split { dir, .. } if *dir == my_dir);
            if merge {
                let Node::Split {
                    children: sub,
                    sizes: sub_sizes,
                    ..
                } = children.remove(i)
                else {
                    unreachable!()
                };
                let share = sizes.remove(i);
                let sub_total: u32 = sub_sizes.iter().map(|s| *s as u32).sum::<u32>().max(1);
                for (j, (node, sub_size)) in sub.into_iter().zip(sub_sizes).enumerate() {
                    let part = ((share as u32 * sub_size as u32) / sub_total).max(1) as u16;
                    children.insert(i + j, node);
                    sizes.insert(i + j, part);
                }
            } else {
                i += 1;
            }
        }
    }

    /// Rotates the children of the container holding `win` by one position.
    pub fn rotate(&mut self, win: WinId) {
        if let Node::Split { children, .. } = self {
            if children
                .iter()
                .any(|c| matches!(c, Node::Leaf { id, .. } if *id == win))
            {
                children.rotate_right(1);
                return;
            }
            for child in children.iter_mut() {
                if child.contains(win) {
                    child.rotate(win);
                    return;
                }
            }
        }
    }

    /// Flips the direction of the container holding `win` and equalizes it.
    pub fn flip(&mut self, win: WinId) {
        if let Node::Split {
            dir,
            children,
            sizes,
        } = self
        {
            if children
                .iter()
                .any(|c| matches!(c, Node::Leaf { id, .. } if *id == win))
            {
                *dir = match dir {
                    SplitDir::Horizontal => SplitDir::Vertical,
                    SplitDir::Vertical => SplitDir::Horizontal,
                };
                sizes.fill(1);
                return;
            }
            for child in children.iter_mut() {
                if child.contains(win) {
                    child.flip(win);
                    return;
                }
            }
        }
    }

    /// Equal shares everywhere.
    pub fn equalize(&mut self) {
        if let Node::Split {
            children, sizes, ..
        } = self
        {
            sizes.fill(1);
            for child in children.iter_mut() {
                child.equalize();
            }
        }
    }

    /// tmux-style resize: pushes the border of `win`'s subtree in `dir` by
    /// `step` cells — the border on that side when it exists, else the
    /// opposite one. Returns true when a border moved.
    pub fn resize(&mut self, win: WinId, dir: WinDir, step: u16, area: Rect) -> bool {
        let axis = match dir {
            WinDir::Left | WinDir::Right => SplitDir::Vertical,
            WinDir::Up | WinDir::Down => SplitDir::Horizontal,
        };
        let grow_after = matches!(dir, WinDir::Right | WinDir::Down);
        self.resize_inner(win, axis, grow_after, step, area)
    }

    fn resize_inner(
        &mut self,
        win: WinId,
        axis: SplitDir,
        grow_after: bool,
        step: u16,
        area: Rect,
    ) -> bool {
        let Node::Split {
            dir,
            children,
            sizes,
        } = self
        else {
            return false;
        };
        let my_dir = *dir;
        // child rects for the recursive descent
        let sep = match my_dir {
            SplitDir::Vertical => (children.len() - 1) as u16,
            SplitDir::Horizontal => 0,
        };
        let total = match my_dir {
            SplitDir::Vertical => area.width.saturating_sub(sep),
            SplitDir::Horizontal => area.height,
        };
        let shares = normalized(sizes, total);
        // deepest container wins: try the child holding the window first
        let idx = children.iter().position(|c| c.contains(win));
        let Some(i) = idx else { return false };
        let child_rect = {
            let mut pos = 0;
            for s in shares.iter().take(i) {
                pos += *s;
            }
            match my_dir {
                SplitDir::Vertical => {
                    Rect::new(area.x + pos + i as u16, area.y, shares[i], area.height)
                }
                SplitDir::Horizontal => Rect::new(area.x, area.y + pos, area.width, shares[i]),
            }
        };
        if children[i].resize_inner(win, axis, grow_after, step, child_rect) {
            return true;
        }
        if my_dir != axis || children.len() < 2 {
            return false;
        }
        let min = match axis {
            SplitDir::Vertical => MIN_WIN_WIDTH,
            SplitDir::Horizontal => MIN_WIN_HEIGHT,
        };
        // the border being pushed: after child i when growing after (or i is
        // last), else before it
        let mut current = normalized(sizes, total);
        let (a, b) = if grow_after {
            if i + 1 < current.len() {
                (i, i + 1) // grow a, shrink b
            } else {
                (i - 1, i) // at the edge: pull the opposite border
            }
        } else if i > 0 {
            (i - 1, i) // shrink a... handled below by sign
        } else {
            (i, i + 1)
        };
        // moving the border between a and b toward b for grow_after,
        // toward a otherwise
        let (grow, shrink) = if grow_after { (a, b) } else { (b, a) };
        let step = step.min(current[shrink].saturating_sub(min));
        if step == 0 {
            return false;
        }
        current[grow] += step;
        current[shrink] -= step;
        *sizes = current;
        true
    }
}

/// Scales stored shares to sum exactly to `total`, each at least 1.
fn normalized(sizes: &[u16], total: u16) -> Vec<u16> {
    let n = sizes.len() as u16;
    if n == 0 {
        return Vec::new();
    }
    if total < n {
        return vec![1; n as usize];
    }
    let sum: u32 = sizes.iter().map(|s| *s as u32).sum::<u32>().max(1);
    let mut out: Vec<u16> = sizes
        .iter()
        .map(|s| (((*s as u32) * total as u32) / sum).max(1) as u16)
        .collect();
    let len = out.len();
    let mut assigned: u16 = out.iter().sum();
    // distribute rounding leftovers left to right (or trim overshoot)
    let mut i = 0;
    while assigned < total {
        out[i % len] += 1;
        assigned += 1;
        i += 1;
    }
    let mut i = 0;
    while assigned > total {
        if out[i % len] > 1 {
            out[i % len] -= 1;
            assigned -= 1;
        }
        i += 1;
    }
    out
}

/// Picks the window reached by moving from `from` in `dir`, preferring the
/// neighbor that overlaps `pref` (the cursor's screen row/column).
pub fn neighbor(rects: &[(WinId, Rect)], from: WinId, dir: WinDir, pref: u16) -> Option<WinId> {
    let cur = rects.iter().find(|(id, _)| *id == from)?.1;
    let candidates = rects.iter().filter(|(id, r)| {
        if *id == from {
            return false;
        }
        match dir {
            // 1-cell tolerance for vertical-split separator columns
            WinDir::Left => r.x + r.width <= cur.x && cur.x - (r.x + r.width) <= 1,
            WinDir::Right => r.x >= cur.x + cur.width && r.x - (cur.x + cur.width) <= 1,
            WinDir::Up => r.y + r.height == cur.y,
            WinDir::Down => r.y == cur.y + cur.height,
        }
    });
    let overlap = |r: &Rect| -> i32 {
        let (lo, hi) = match dir {
            WinDir::Left | WinDir::Right => (r.y, r.y + r.height),
            WinDir::Up | WinDir::Down => (r.x, r.x + r.width),
        };
        if pref >= lo && pref < hi {
            i32::MAX // contains the cursor projection
        } else {
            -((pref as i32 - lo as i32)
                .abs()
                .min((pref as i32 - hi as i32).abs()))
        }
    };
    candidates
        .max_by_key(|(_, r)| overlap(r))
        .map(|(id, _)| *id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> Rect {
        Rect::new(0, 0, 80, 22)
    }

    fn rects(node: &Node) -> Vec<(WinId, Rect)> {
        let mut out = Vec::new();
        node.layout(area(), &mut out);
        out
    }

    #[test]
    fn single_leaf_fills_area() {
        let node = Node::leaf(1, None);
        assert_eq!(rects(&node), vec![(1, area())]);
    }

    #[test]
    fn vsplit_shares_width_with_separator() {
        let mut node = Node::leaf(1, None);
        node.split(
            1,
            SplitDir::Vertical,
            Node::leaf(2, Some(WinState::default())),
        );
        let r = rects(&node);
        assert_eq!(r.len(), 2);
        let (w1, w2) = (r[0].1.width, r[1].1.width);
        assert_eq!(w1 + w2 + 1, 80); // one separator column
        assert_eq!(r[1].1.x, r[0].1.width + 1);
    }

    #[test]
    fn hsplit_shares_height_exactly() {
        let mut node = Node::leaf(1, None);
        node.split(
            1,
            SplitDir::Horizontal,
            Node::leaf(2, Some(WinState::default())),
        );
        let r = rects(&node);
        assert_eq!(r[0].1.height + r[1].1.height, 22);
    }

    #[test]
    fn same_direction_split_flattens() {
        let mut node = Node::leaf(1, None);
        node.split(
            1,
            SplitDir::Vertical,
            Node::leaf(2, Some(WinState::default())),
        );
        node.split(
            1,
            SplitDir::Vertical,
            Node::leaf(3, Some(WinState::default())),
        );
        match &node {
            Node::Split { children, .. } => assert_eq!(children.len(), 3),
            _ => panic!("expected flat split"),
        }
        assert_eq!(node.window_ids(), vec![1, 3, 2]);
    }

    #[test]
    fn close_collapses_singletons() {
        let mut node = Node::leaf(1, None);
        node.split(
            1,
            SplitDir::Vertical,
            Node::leaf(2, Some(WinState::default())),
        );
        node.split(
            1,
            SplitDir::Horizontal,
            Node::leaf(3, Some(WinState::default())),
        );
        assert_eq!(node.window_count(), 3);
        node.close(3);
        assert_eq!(node.window_count(), 2);
        node.close(2);
        assert!(matches!(node, Node::Leaf { id: 1, .. }));
    }

    #[test]
    fn neighbor_picks_overlapping_window() {
        // [1 | 2] over [3] — from 3 moving up with cursor on the right half
        let rects = vec![
            (1, Rect::new(0, 0, 40, 11)),
            (2, Rect::new(41, 0, 39, 11)),
            (3, Rect::new(0, 11, 80, 11)),
        ];
        assert_eq!(neighbor(&rects, 3, WinDir::Up, 60), Some(2));
        assert_eq!(neighbor(&rects, 3, WinDir::Up, 10), Some(1));
        assert_eq!(neighbor(&rects, 1, WinDir::Right, 5), Some(2));
        assert_eq!(neighbor(&rects, 2, WinDir::Left, 5), Some(1));
        assert_eq!(neighbor(&rects, 1, WinDir::Down, 20), Some(3));
        assert_eq!(neighbor(&rects, 1, WinDir::Up, 5), None);
    }

    #[test]
    fn normalized_sums_to_total() {
        assert_eq!(normalized(&[1, 1, 1], 80).iter().sum::<u16>(), 80);
        assert_eq!(normalized(&[3, 1], 7).iter().sum::<u16>(), 7);
        assert_eq!(normalized(&[10, 10], 21), vec![11, 10]);
    }
}
