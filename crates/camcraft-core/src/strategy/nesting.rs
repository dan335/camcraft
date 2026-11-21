//! Passes in regions that nest, cut region by region: what pocket clearing, flat
//! finishing and scallop finishing share.
//!
//! Each region holds its own passes and the regions a step further in, inside it.
//! A region's subtree is cut before the tool moves to the next, so it finishes one
//! arm of an area before the next and never goes back to a pass it has left: the
//! passes it cuts before its children, then its children, the nearest next, then
//! the passes it cuts after them. Seen as rings nesting in one another, from the
//! boundary in a region's rings round its outside come before what is inside them
//! and its rings round its holes after; from the middle out, the other way. The walk keeps a stack of its own, as a tree is as deep as the
//! area is wide over the step.

use glam::DVec2;

/// Which way round passes in nested regions are cut.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Order {
    /// From the boundary in. The default.
    #[default]
    OutsideIn,
    /// Each region's innermost passes first, out to the boundary.
    InsideOut,
}

/// Regions' passes, as the regions nest.
#[derive(Debug)]
pub(crate) struct Nested<P> {
    nodes: Vec<Node<P>>,
}

#[derive(Debug)]
struct Node<P> {
    /// Cut before its children.
    before: Vec<P>,
    /// Cut after them.
    after: Vec<P>,
    children: Vec<usize>,
    /// Where the region's subtree starts cutting, to go to the nearest by.
    anchor: Option<DVec2>,
}

/// What cuts a region's passes, from where the tool is.
pub(crate) trait Cuts<P> {
    /// What cutting may fail with.
    type Error;
    /// Where the tool is, if anywhere yet.
    fn at(&self) -> Option<DVec2>;
    /// Cut one region's passes.
    fn cut(&mut self, passes: Vec<P>) -> Result<(), Self::Error>;
}

impl<P> Nested<P> {
    pub(crate) fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    /// A region with passes cut `before` its children and `after` them, and
    /// no children yet.
    pub(crate) fn push(&mut self, (before, after): (Vec<P>, Vec<P>)) -> usize {
        self.nodes.push(Node {
            before,
            after,
            children: Vec::new(),
            anchor: None,
        });
        self.nodes.len() - 1
    }

    /// Region `child` nested in `parent`.
    pub(crate) fn nest(&mut self, parent: usize, child: usize) {
        if let Some(node) = self.nodes.get_mut(parent) {
            node.children.push(child);
        }
    }

    /// Where each region's subtree starts: its first pass's `start`, of those
    /// before its children or else after them, or its first child's that has
    /// one. Every child is settled before its parent.
    pub(crate) fn settle(&mut self, start: impl Fn(&P) -> Option<DVec2>) {
        let mut done = vec![false; self.nodes.len()];
        for root in 0..self.nodes.len() {
            let mut stack = vec![(root, false)];
            while let Some((n, children_done)) = stack.pop() {
                if done.get(n).copied().unwrap_or(true) {
                    continue;
                }
                let Some(node) = self.nodes.get(n) else {
                    continue;
                };
                if !children_done {
                    stack.push((n, true));
                    stack.extend(node.children.iter().map(|&c| (c, false)));
                    continue;
                }
                let first = node.before.first().or(node.after.first());
                let anchor = first.and_then(&start).or_else(|| {
                    node.children
                        .iter()
                        .find_map(|&c| self.nodes.get(c).and_then(|c| c.anchor))
                });
                if let Some(node) = self.nodes.get_mut(n) {
                    node.anchor = anchor;
                }
                if let Some(d) = done.get_mut(n) {
                    *d = true;
                }
            }
        }
    }

    /// Where region `n`'s subtree starts, once settled.
    pub(crate) fn anchor(&self, n: usize) -> Option<DVec2> {
        self.nodes.get(n).and_then(|node| node.anchor)
    }

    /// Cut the subtree at `root` with `cutter`.
    pub(crate) fn cut<C: Cuts<P>>(&mut self, root: usize, cutter: &mut C) -> Result<(), C::Error> {
        /// A region on the way down: its passes before its children while
        /// still to cut, the children left, and its passes after them.
        type Open<P> = (Vec<P>, Vec<usize>, Vec<P>);
        let take = |nodes: &mut Vec<Node<P>>, n: usize| -> Open<P> {
            nodes
                .get_mut(n)
                .map(|node| {
                    (
                        std::mem::take(&mut node.before),
                        std::mem::take(&mut node.children),
                        std::mem::take(&mut node.after),
                    )
                })
                .unwrap_or_default()
        };
        let mut stack: Vec<Open<P>> = vec![take(&mut self.nodes, root)];
        while let Some((before, left, after)) = stack.last_mut() {
            if !before.is_empty() {
                cutter.cut(std::mem::take(before))?;
                continue;
            }
            if left.is_empty() {
                let after = std::mem::take(after);
                stack.pop();
                if !after.is_empty() {
                    cutter.cut(after)?;
                }
                continue;
            }
            let at = cutter.at();
            let k = nearest(left, |&c| distance(self.anchor(c), at));
            let child = left.remove(k);
            stack.push(take(&mut self.nodes, child));
        }
        Ok(())
    }
}

/// Which of `items` is nearest, by `distance`, ties to the earlier.
pub(crate) fn nearest<T>(items: &[T], distance: impl Fn(&T) -> f64) -> usize {
    items
        .iter()
        .enumerate()
        .fold((0, f64::MAX), |best, (i, item)| {
            let d = distance(item);
            if d < best.1 { (i, d) } else { best }
        })
        .0
}

/// The nearest of `items` by `distance`, taken out of them, ties to the
/// earlier; `None` once there are none. Taken one at a time, from wherever
/// the last left the tool, they are in nearest-next order.
pub(crate) fn take_nearest<T>(items: &mut Vec<T>, distance: impl Fn(&T) -> f64) -> Option<T> {
    if items.is_empty() {
        return None;
    }
    Some(items.remove(nearest(items, distance)))
}

/// `items` in nearest-next order: from `from`, always the one starting nearest
/// where the last ended, seen from above, `ends` giving where each starts and
/// ends; ties to the earlier.
pub(crate) fn nearest_next<T>(
    mut items: Vec<T>,
    from: DVec2,
    ends: impl Fn(&T) -> (DVec2, DVec2),
) -> Vec<T> {
    let mut out = Vec::with_capacity(items.len());
    let mut at = from;
    while let Some(item) = take_nearest(&mut items, |i| at.distance(ends(i).0)) {
        at = ends(&item).1;
        out.push(item);
    }
    out
}

/// How far `to` is from the tool at `at`: from nowhere everything is as near, and
/// nowhere is as far as can be.
pub(crate) fn distance(to: Option<DVec2>, at: Option<DVec2>) -> f64 {
    match (to, at) {
        (Some(to), Some(at)) => to.distance(at),
        (_, None) => 0.0,
        (None, Some(_)) => f64::MAX,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use glam::dvec2;

    /// Collects what it is asked to cut, the tool ending at each region's pass.
    struct Log(Vec<i32>, Option<DVec2>);

    impl Cuts<(i32, DVec2)> for Log {
        type Error = ();
        fn at(&self) -> Option<DVec2> {
            self.1
        }
        fn cut(&mut self, passes: Vec<(i32, DVec2)>) -> Result<(), ()> {
            for (id, at) in passes {
                self.0.push(id);
                self.1 = Some(at);
            }
            Ok(())
        }
    }

    /// A root with a near child and a far one, the far one with a child of its
    /// own: the root's pass, then the nearer child's subtree, then the other's.
    fn tree(children_first: bool) -> Nested<(i32, DVec2)> {
        let mut t = Nested::new();
        let passes = |p: (i32, DVec2)| {
            if children_first {
                (Vec::new(), vec![p])
            } else {
                (vec![p], Vec::new())
            }
        };
        let root = t.push(passes((0, dvec2(0.0, 0.0))));
        let far = t.push(passes((1, dvec2(10.0, 0.0))));
        let near = t.push(passes((2, dvec2(1.0, 0.0))));
        let deep = t.push(passes((3, dvec2(11.0, 0.0))));
        t.nest(root, far);
        t.nest(root, near);
        t.nest(far, deep);
        t.settle(|p| Some(p.1));
        t
    }

    #[test]
    fn parents_first_goes_region_by_region_nearest_next() {
        let mut log = Log(Vec::new(), None);
        tree(false).cut(0, &mut log).unwrap();
        assert_eq!(log.0, vec![0, 2, 1, 3]);
    }

    #[test]
    fn children_first_cuts_each_region_after_its_children() {
        let mut log = Log(Vec::new(), None);
        tree(true).cut(0, &mut log).unwrap();
        // From nowhere the first child is nearest; its child before it.
        assert_eq!(log.0, vec![3, 1, 2, 0]);
    }

    /// Rings round a region's outside before its children and round its hole
    /// after them: the outer, the child, the hole's.
    #[test]
    fn a_region_cuts_some_passes_before_its_children_and_some_after() {
        let mut t: Nested<(i32, DVec2)> = Nested::new();
        let root = t.push((vec![(0, dvec2(0.0, 0.0))], vec![(2, dvec2(3.0, 0.0))]));
        let child = t.push((vec![(1, dvec2(1.0, 0.0))], Vec::new()));
        t.nest(root, child);
        t.settle(|p| Some(p.1));
        let mut log = Log(Vec::new(), None);
        t.cut(root, &mut log).unwrap();
        assert_eq!(log.0, vec![0, 1, 2]);
    }

    #[test]
    fn a_region_with_no_passes_starts_where_its_first_child_does() {
        let mut t: Nested<(i32, DVec2)> = Nested::new();
        let root = t.push((Vec::new(), Vec::new()));
        let child = t.push((vec![(1, dvec2(5.0, 5.0))], Vec::new()));
        t.nest(root, child);
        t.settle(|p| Some(p.1));
        assert_eq!(t.anchor(root), Some(dvec2(5.0, 5.0)));
    }
}
