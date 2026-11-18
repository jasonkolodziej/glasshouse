//! How split tabs are arranged: a tree of panes, each split a row (side by side) or a column
//! (stacked) whose children share its length by weight, and where a dragged tab lands.
//!
//! The tree is plain data, so every rearrangement is tested here without a window.

/// The direction a split lays its children out in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// Left to right: panes side by side.
    Row,
    /// Top to bottom: panes stacked.
    Column,
}

/// Where on a pane a dragged tab is dropped: an edge puts it beside the pane on that side, the
/// middle swaps the two.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
    Center,
}

impl Edge {
    /// The split an edge drop makes, and whether the new pane goes before the target.
    fn placement(self) -> Option<(Axis, bool)> {
        match self {
            Edge::Left => Some((Axis::Row, true)),
            Edge::Right => Some((Axis::Row, false)),
            Edge::Top => Some((Axis::Column, true)),
            Edge::Bottom => Some((Axis::Column, false)),
            Edge::Center => None,
        }
    }
}

/// Panes by tab id. A split always has two or more children; each child's weight is its share
/// of the split's length.
#[derive(Clone, Debug, PartialEq)]
pub enum PaneTree {
    Leaf(u64),
    Split {
        axis: Axis,
        children: Vec<(PaneTree, f32)>,
    },
}

/// The narrowest a pane may be dragged, in points.
pub const MIN_PANE: f32 = 80.0;

impl PaneTree {
    /// `ids` in one row or column, equal shares. `None` for fewer than two.
    pub fn flat(axis: Axis, ids: &[u64]) -> Option<PaneTree> {
        (ids.len() > 1).then(|| PaneTree::Split {
            axis,
            children: ids.iter().map(|id| (PaneTree::Leaf(*id), 1.0)).collect(),
        })
    }

    /// Every tab in the tree, in reading order (left to right, top to bottom).
    pub fn leaves(&self) -> Vec<u64> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect(&self, out: &mut Vec<u64>) {
        match self {
            PaneTree::Leaf(id) => out.push(*id),
            PaneTree::Split { children, .. } => {
                for (child, _) in children {
                    child.collect(out);
                }
            }
        }
    }

    pub fn contains(&self, id: u64) -> bool {
        match self {
            PaneTree::Leaf(leaf) => *leaf == id,
            PaneTree::Split { children, .. } => children.iter().any(|(c, _)| c.contains(id)),
        }
    }

    /// One row or column of tabs, nothing nested: what ⌘D and ⇧⌘D make.
    pub fn flat_axis(&self) -> Option<Axis> {
        match self {
            PaneTree::Split { axis, children }
                if children.iter().all(|(c, _)| matches!(c, PaneTree::Leaf(_))) =>
            {
                Some(*axis)
            }
            _ => None,
        }
    }

    /// The tree without `id`: a split left with one child becomes that child, and a split
    /// inside one of the same axis merges into it. `None` when nothing is left.
    pub fn without(self, id: u64) -> Option<PaneTree> {
        match self {
            PaneTree::Leaf(leaf) if leaf == id => None,
            PaneTree::Leaf(_) => Some(self),
            PaneTree::Split { axis, children } => {
                let children = children
                    .into_iter()
                    .filter_map(|(c, w)| c.without(id).map(|c| (c, w)))
                    .collect();
                PaneTree::split(axis, children)
            }
        }
    }

    /// A split of `children`, simplified: none → `None`, one → that child, and a same-axis
    /// child's children taken in, each scaled to the share the child had.
    fn split(axis: Axis, children: Vec<(PaneTree, f32)>) -> Option<PaneTree> {
        let mut flat = Vec::with_capacity(children.len());
        for (child, weight) in children {
            match child {
                PaneTree::Split {
                    axis: inner,
                    children: grandchildren,
                } if inner == axis => {
                    let total: f32 = grandchildren.iter().map(|(_, w)| w).sum();
                    flat.extend(
                        grandchildren
                            .into_iter()
                            .map(|(g, w)| (g, weight * w / total.max(f32::EPSILON))),
                    );
                }
                other => flat.push((other, weight)),
            }
        }
        match flat.len() {
            0 => None,
            1 => flat.pop().map(|(child, _)| child),
            _ => Some(PaneTree::Split {
                axis,
                children: flat,
            }),
        }
    }

    /// Puts `id` beside `target` on `edge`'s side. Beside a pane in a split of the same axis it
    /// joins that split, taking half the target's share; otherwise the target becomes a new
    /// split of the two. A centre drop swaps them if `id` is already in the tree, else puts
    /// `id` in the target's place. Returns whether the tree changed.
    pub fn place(&mut self, target: u64, id: u64, edge: Edge) -> bool {
        if target == id || !self.contains(target) {
            return false;
        }
        let Some((axis, before)) = edge.placement() else {
            if self.contains(id) {
                self.swap(target, id);
            } else {
                self.replace(target, id);
            }
            return true;
        };
        if self.contains(id) {
            let Some(rest) = std::mem::replace(self, PaneTree::Leaf(target)).without(id) else {
                return false;
            };
            *self = rest;
        }
        self.insert(target, id, axis, before);
        true
    }

    fn insert(&mut self, target: u64, id: u64, axis: Axis, before: bool) {
        // A split of `axis` holding the target directly takes the new pane as a sibling.
        if let PaneTree::Split {
            axis: own,
            children,
        } = self
            && *own == axis
            && let Some(i) = children
                .iter()
                .position(|(c, _)| *c == PaneTree::Leaf(target))
        {
            let half = children[i].1 / 2.0;
            children[i].1 = half;
            children.insert(if before { i } else { i + 1 }, (PaneTree::Leaf(id), half));
            return;
        }
        match self {
            PaneTree::Leaf(leaf) if *leaf == target => {
                let pair = [(PaneTree::Leaf(id), 1.0), (PaneTree::Leaf(target), 1.0)];
                let children = if before {
                    pair.to_vec()
                } else {
                    pair.into_iter().rev().collect()
                };
                *self = PaneTree::Split { axis, children };
            }
            PaneTree::Leaf(_) => {}
            PaneTree::Split { children, .. } => {
                for (child, _) in children.iter_mut() {
                    if child.contains(target) {
                        child.insert(target, id, axis, before);
                        return;
                    }
                }
            }
        }
    }

    fn swap(&mut self, a: u64, b: u64) {
        match self {
            PaneTree::Leaf(id) if *id == a => *id = b,
            PaneTree::Leaf(id) if *id == b => *id = a,
            PaneTree::Leaf(_) => {}
            PaneTree::Split { children, .. } => {
                for (child, _) in children {
                    child.swap(a, b);
                }
            }
        }
    }

    fn replace(&mut self, old: u64, new: u64) {
        match self {
            PaneTree::Leaf(id) if *id == old => *id = new,
            PaneTree::Leaf(_) => {}
            PaneTree::Split { children, .. } => {
                for (child, _) in children {
                    child.replace(old, new);
                }
            }
        }
    }

    /// Puts `id` beside `target` along the split that holds `target` (to its right, or below
    /// it), as a new tab opened from a split pane does.
    pub fn add_beside(&mut self, target: u64, id: u64) {
        let axis = self.parent_axis(target).unwrap_or(Axis::Row);
        self.insert(target, id, axis, false);
    }

    /// The axis of the split directly holding `id`.
    fn parent_axis(&self, id: u64) -> Option<Axis> {
        let PaneTree::Split { axis, children } = self else {
            return None;
        };
        if children.iter().any(|(c, _)| *c == PaneTree::Leaf(id)) {
            return Some(*axis);
        }
        children.iter().find_map(|(c, _)| c.parent_axis(id))
    }

    /// The split at `path` (child indices from the root).
    pub fn node_mut(&mut self, path: &[usize]) -> Option<&mut PaneTree> {
        match path.split_first() {
            None => Some(self),
            Some((i, rest)) => match self {
                PaneTree::Split { children, .. } => children.get_mut(*i)?.0.node_mut(rest),
                PaneTree::Leaf(_) => None,
            },
        }
    }

    pub fn node(&self, path: &[usize]) -> Option<&PaneTree> {
        match path.split_first() {
            None => Some(self),
            Some((i, rest)) => match self {
                PaneTree::Split { children, .. } => children.get(*i)?.0.node(rest),
                PaneTree::Leaf(_) => None,
            },
        }
    }

    /// Orders a flat split's panes as `order` lists them (the tab strip after a reorder).
    pub fn follow_order(&mut self, order: &[u64]) {
        if self.flat_axis().is_none() {
            return;
        }
        if let PaneTree::Split { children, .. } = self {
            children.sort_by_key(|(c, _)| match c {
                PaneTree::Leaf(id) => order.iter().position(|o| o == id).unwrap_or(usize::MAX),
                PaneTree::Split { .. } => usize::MAX,
            });
        }
    }
}

/// The two weights either side of divider `i` after dragging it `delta` points along a split
/// `length` points long, from `start` = (left or top weight, right or bottom weight). Neither
/// pane gets narrower than [`MIN_PANE`] (or half the pair, if the pair is narrower than two).
pub fn drag_weights(start: (f32, f32), total: f32, length: f32, delta: f32) -> (f32, f32) {
    if length <= 0.0 || total <= 0.0 {
        return start;
    }
    let per_point = total / length;
    let pair = start.0 + start.1;
    let min = (MIN_PANE * per_point).min(pair / 2.0);
    let first = (start.0 + delta * per_point).clamp(min, pair - min);
    (first, pair - first)
}

/// Where a tab dragged to `point` (window points) would land.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DropTarget {
    /// Into the tab strip, before the tab at this index (or at the end).
    Strip(usize),
    /// Onto a pane, at an edge or the middle.
    Pane(u64, Edge),
}

/// What's under `point`: the strip (an insertion index from the tabs' centres), else a pane on
/// screen. A pane's middle fifth either way is its centre; elsewhere, the nearest edge.
pub fn drop_target(
    point: [f32; 2],
    strip: [f32; 4],
    tabs: &[[f32; 4]],
    panes: &[(u64, [f32; 4])],
) -> Option<DropTarget> {
    let [x, y] = point;
    let inside = |r: &[f32; 4]| x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3];
    if inside(&strip) {
        let index = tabs.iter().filter(|t| t[0] + t[2] / 2.0 < x).count();
        return Some(DropTarget::Strip(index));
    }
    let (id, r) = panes.iter().find(|(_, r)| inside(r))?;
    let fx = (x - r[0]) / r[2].max(1.0);
    let fy = (y - r[1]) / r[3].max(1.0);
    if (0.3..0.7).contains(&fx) && (0.3..0.7).contains(&fy) {
        return Some(DropTarget::Pane(*id, Edge::Center));
    }
    let edges = [
        (fx, Edge::Left),
        (1.0 - fx, Edge::Right),
        (fy, Edge::Top),
        (1.0 - fy, Edge::Bottom),
    ];
    let edge = edges
        .into_iter()
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map_or(Edge::Center, |(_, e)| e);
    Some(DropTarget::Pane(*id, edge))
}

#[cfg(test)]
mod tests {
    use super::*;

    use PaneTree::Leaf;

    fn split(axis: Axis, children: Vec<PaneTree>) -> PaneTree {
        PaneTree::Split {
            axis,
            children: children.into_iter().map(|c| (c, 1.0)).collect(),
        }
    }

    #[test]
    fn flat_should_need_two_tabs() {
        assert_eq!(PaneTree::flat(Axis::Row, &[1]), None);
        assert_eq!(
            PaneTree::flat(Axis::Row, &[1, 2]),
            Some(split(Axis::Row, vec![Leaf(1), Leaf(2)]))
        );
    }

    #[test]
    fn dropping_on_an_edge_should_split_a_lone_pane() {
        let mut t = Leaf(1);
        assert!(t.place(1, 2, Edge::Right));
        assert_eq!(t, split(Axis::Row, vec![Leaf(1), Leaf(2)]));
        let mut t = Leaf(1);
        t.place(1, 2, Edge::Top);
        assert_eq!(t, split(Axis::Column, vec![Leaf(2), Leaf(1)]));
    }

    #[test]
    fn dropping_along_the_split_should_join_it_with_half_the_targets_share() {
        let mut t = split(Axis::Row, vec![Leaf(1), Leaf(2)]);
        t.place(1, 3, Edge::Right);
        assert_eq!(
            t,
            PaneTree::Split {
                axis: Axis::Row,
                children: vec![(Leaf(1), 0.5), (Leaf(3), 0.5), (Leaf(2), 1.0)],
            }
        );
    }

    #[test]
    fn dropping_across_the_split_should_nest_a_new_one() {
        let mut t = split(Axis::Row, vec![Leaf(1), Leaf(2)]);
        t.place(2, 3, Edge::Bottom);
        assert_eq!(
            t,
            split(
                Axis::Row,
                vec![Leaf(1), split(Axis::Column, vec![Leaf(2), Leaf(3)])]
            )
        );
        assert_eq!(t.leaves(), [1, 2, 3]);
    }

    #[test]
    fn moving_a_pane_should_take_it_out_of_its_old_place() {
        let mut t = split(Axis::Row, vec![Leaf(1), Leaf(2), Leaf(3)]);
        t.place(1, 3, Edge::Top);
        assert_eq!(
            t,
            split(
                Axis::Row,
                vec![split(Axis::Column, vec![Leaf(3), Leaf(1)]), Leaf(2)]
            )
        );
    }

    #[test]
    fn moving_one_of_two_panes_to_the_others_side_should_keep_both() {
        let mut t = split(Axis::Row, vec![Leaf(1), Leaf(2)]);
        assert!(t.place(1, 2, Edge::Left));
        assert_eq!(t, split(Axis::Row, vec![Leaf(2), Leaf(1)]));
        assert!(t.place(2, 1, Edge::Bottom));
        assert_eq!(t, split(Axis::Column, vec![Leaf(2), Leaf(1)]));
    }

    #[test]
    fn dropping_in_the_middle_should_swap_or_replace() {
        let mut t = split(Axis::Row, vec![Leaf(1), Leaf(2)]);
        t.place(1, 2, Edge::Center);
        assert_eq!(t.leaves(), [2, 1]);
        t.place(1, 9, Edge::Center);
        assert_eq!(t.leaves(), [2, 9]);
    }

    #[test]
    fn dropping_a_pane_on_itself_should_change_nothing() {
        let mut t = split(Axis::Row, vec![Leaf(1), Leaf(2)]);
        assert!(!t.place(1, 1, Edge::Left));
        assert!(!t.place(7, 1, Edge::Left), "a target outside the tree");
    }

    #[test]
    fn without_should_collapse_and_merge_splits() {
        let t = split(
            Axis::Row,
            vec![
                Leaf(1),
                split(
                    Axis::Column,
                    vec![Leaf(2), split(Axis::Row, vec![Leaf(3), Leaf(4)])],
                ),
            ],
        );
        // The column is left with one row, which merges into the outer row at its share.
        let t = t.without(2).expect("tabs left");
        assert_eq!(
            t,
            PaneTree::Split {
                axis: Axis::Row,
                children: vec![(Leaf(1), 1.0), (Leaf(3), 0.5), (Leaf(4), 0.5)],
            }
        );
        let t = t.without(3).and_then(|t| t.without(4)).expect("one left");
        assert_eq!(t, Leaf(1));
        assert_eq!(t.without(1), None);
    }

    #[test]
    fn a_new_tab_should_open_beside_the_focused_pane_along_its_split() {
        let mut t = split(
            Axis::Row,
            vec![Leaf(1), split(Axis::Column, vec![Leaf(2), Leaf(3)])],
        );
        t.add_beside(2, 4);
        assert_eq!(t.leaves(), [1, 2, 4, 3]);
        t.add_beside(1, 5);
        assert_eq!(t.leaves(), [1, 5, 2, 4, 3]);
    }

    #[test]
    fn follow_order_should_reorder_only_a_flat_split() {
        let mut t = split(Axis::Row, vec![Leaf(1), Leaf(2), Leaf(3)]);
        t.follow_order(&[3, 1, 2]);
        assert_eq!(t.leaves(), [3, 1, 2]);
        let mut nested = split(
            Axis::Row,
            vec![Leaf(1), split(Axis::Column, vec![Leaf(2), Leaf(3)])],
        );
        nested.follow_order(&[3, 2, 1]);
        assert_eq!(nested.leaves(), [1, 2, 3]);
    }

    #[test]
    fn drag_weights_should_follow_the_pointer_and_stop_at_the_minimum() {
        // Two equal panes in 800 points: 1 weight = 400 points.
        assert_eq!(drag_weights((1.0, 1.0), 2.0, 800.0, 100.0), (1.25, 0.75));
        let (a, b) = drag_weights((1.0, 1.0), 2.0, 800.0, 10_000.0);
        assert!((b - MIN_PANE / 400.0).abs() < 1e-5 && (a + b - 2.0).abs() < 1e-5);
        assert_eq!(drag_weights((1.0, 1.0), 2.0, 0.0, 50.0), (1.0, 1.0));
    }

    #[test]
    fn drop_target_should_read_the_strip_then_the_nearest_pane_edge() {
        let strip = [0.0, 0.0, 600.0, 30.0];
        let tabs = [[10.0, 0.0, 100.0, 30.0], [110.0, 0.0, 100.0, 30.0]];
        let panes = [
            (1, [0.0, 40.0, 300.0, 400.0]),
            (2, [300.0, 40.0, 300.0, 400.0]),
        ];
        let at = |x, y| drop_target([x, y], strip, &tabs, &panes);
        assert_eq!(at(20.0, 10.0), Some(DropTarget::Strip(0)));
        assert_eq!(at(100.0, 10.0), Some(DropTarget::Strip(1)));
        assert_eq!(at(500.0, 10.0), Some(DropTarget::Strip(2)));
        assert_eq!(at(150.0, 240.0), Some(DropTarget::Pane(1, Edge::Center)));
        assert_eq!(at(20.0, 240.0), Some(DropTarget::Pane(1, Edge::Left)));
        assert_eq!(at(580.0, 240.0), Some(DropTarget::Pane(2, Edge::Right)));
        assert_eq!(at(450.0, 60.0), Some(DropTarget::Pane(2, Edge::Top)));
        assert_eq!(at(450.0, 430.0), Some(DropTarget::Pane(2, Edge::Bottom)));
        assert_eq!(at(700.0, 240.0), None);
    }
}
