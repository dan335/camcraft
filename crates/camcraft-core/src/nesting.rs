//! Passes in regions that nest, cut region by region: what pocket clearing and
//! scallop finishing share.
//!
//! Each region holds its own passes and the regions a step further in, inside it.
//! A region's subtree is cut before the tool moves to the next, so it finishes one
//! arm of an area before the next and never goes back to a pass it has left: its
//! own passes before its children's, or after them, and of its children the
//! nearest next. The walk keeps a stack of its own, as a tree is as deep as the
//! area is wide over the step.

use glam::DVec2;

/// Which way round passes in nested regions are cut.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Order {
    /// From the boundary in. Fusion's default.
    #[default]
    OutsideIn,
    /// Each region's innermost passes first, out to the boundary.
    InsideOut,
}

/// Regions' passes, as the regions nest.
#[derive(Debug)]
pub(crate) struct Nested<P> {
    nodes: Vec<Node<P>>,
    /// Whether a region's children are cut before its own passes.
    children_first: bool,
}

#[derive(Debug)]
struct Node<P> {
    passes: Vec<P>,
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
    pub(crate) fn new(children_first: bool) -> Self {
        Self {
            nodes: Vec::new(),
            children_first,
        }
    }

    /// A region with `passes`, and no children yet.
    pub(crate) fn push(&mut self, passes: Vec<P>) -> usize {
        self.nodes.push(Node {
            passes,
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

    /// Where each region's subtree starts: its first pass's `start`, or its first
    /// child's that has one. Every child is settled before its parent.
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
                let anchor = node.passes.first().and_then(&start).or_else(|| {
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
        let take = |nodes: &mut Vec<Node<P>>, n: usize| {
            nodes
                .get_mut(n)
                .map(|node| {
                    (
                        std::mem::take(&mut node.passes),
                        std::mem::take(&mut node.children),
                    )
                })
                .unwrap_or_default()
        };
        // Each region on the way down: its passes while still to cut, and the
        // children left.
        let (passes, children) = take(&mut self.nodes, root);
        let mut stack: Vec<(Option<Vec<P>>, Vec<usize>)> = vec![(Some(passes), children)];
        while let Some((passes, left)) = stack.last_mut() {
            if passes.is_some() && (!self.children_first || left.is_empty()) {
                if let Some(passes) = passes.take() {
                    cutter.cut(passes)?;
                }
                continue;
            }
            if left.is_empty() {
                stack.pop();
                continue;
            }
            let at = cutter.at();
            let k = nearest(left, |&c| distance(self.anchor(c), at));
            let child = left.remove(k);
            let (passes, children) = take(&mut self.nodes, child);
            stack.push((Some(passes), children));
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
        let mut t = Nested::new(children_first);
        let root = t.push(vec![(0, dvec2(0.0, 0.0))]);
        let far = t.push(vec![(1, dvec2(10.0, 0.0))]);
        let near = t.push(vec![(2, dvec2(1.0, 0.0))]);
        let deep = t.push(vec![(3, dvec2(11.0, 0.0))]);
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

    #[test]
    fn a_region_with_no_passes_starts_where_its_first_child_does() {
        let mut t: Nested<(i32, DVec2)> = Nested::new(false);
        let root = t.push(Vec::new());
        let child = t.push(vec![(1, dvec2(5.0, 5.0))]);
        t.nest(root, child);
        t.settle(|p| Some(p.1));
        assert_eq!(t.anchor(root), Some(dvec2(5.0, 5.0)));
    }
}
