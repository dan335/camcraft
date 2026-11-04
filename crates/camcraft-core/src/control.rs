//! A run watched from outside it: stopped, from an operation on, and how far
//! each operation has got.
//!
//! Whoever starts a run holds its [`Control`]; each operation's work is given a
//! [`Watch`] on it, which it asks whether to stop as it goes, and tells how far
//! it has got. Stopping is from an operation on: one after an operation that is
//! being worked out can be stopped while that one goes on, since what comes
//! before an operation never depends on it. A stopped operation's work ends at
//! the next point it asks, and the run with it; everything made before it
//! stands.
//!
//! A [`Watch`] by default watches nothing: it never stops, and what it is told
//! goes nowhere. So a strategy run on its own, as the tests and the command
//! line run them, behaves as it always has.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

/// How finely how far an operation has got is kept: in ten-thousandths, the
/// whole being this many.
const WHOLE: u32 = 10_000;

/// [`WHOLE`], to scale by.
const SHARES: f64 = WHOLE as f64;

/// `done` of `of` equal pieces of work, as a share of it from 0 to 1: what a
/// piece adds as it is done ([`Watch::add`]), or how far a count has got
/// ([`Watch::report`]). Of none, all of it.
pub fn share(done: usize, of: usize) -> f64 {
    if of == 0 {
        return 1.0;
    }
    // Counts of pieces of work are far below where an f64 loses whole numbers.
    #[allow(clippy::cast_precision_loss)]
    let (done, of) = (done as f64, of as f64);
    (done / of).clamp(0.0, 1.0)
}

/// What a [`Control`] and its watches share.
#[derive(Debug)]
struct Shared {
    /// The first operation not to be made: every one from it on stops. Past
    /// every operation while none is to stop.
    stop_from: AtomicUsize,
    /// How far each operation has got, in [`SHARES`], by its place.
    done: Vec<AtomicU32>,
}

/// A run, as whoever started it holds it: to stop it, and to see how far each
/// operation has got.
#[derive(Debug, Clone)]
pub struct Control {
    shared: Arc<Shared>,
}

impl Control {
    /// A control for a run of `operations` operations, none stopped.
    pub fn new(operations: usize) -> Self {
        Self {
            shared: Arc::new(Shared {
                stop_from: AtomicUsize::new(usize::MAX),
                done: (0..operations).map(|_| AtomicU32::new(0)).collect(),
            }),
        }
    }

    /// Stop operation `op` and every one after it; the ones before go on. Asked
    /// again, it stops from the earlier of the two.
    pub fn stop_from(&self, op: usize) {
        self.shared.stop_from.fetch_min(op, Ordering::Relaxed);
    }

    /// Stop the whole run.
    pub fn stop(&self) {
        self.stop_from(0);
    }

    /// The first operation stopped, if any is.
    pub fn stopped_from(&self) -> Option<usize> {
        let from = self.shared.stop_from.load(Ordering::Relaxed);
        (from != usize::MAX).then_some(from)
    }

    /// How far operation `op` has got, from 0 to 1, as its work has said: 0
    /// for one that has said nothing.
    pub fn done(&self, op: usize) -> f64 {
        self.shared
            .done
            .get(op)
            .map_or(0.0, |d| f64::from(d.load(Ordering::Relaxed)) / SHARES)
    }

    /// The watch operation `op`'s work is given.
    pub fn watch(&self, op: usize) -> Watch {
        Watch {
            shared: Some(Arc::clone(&self.shared)),
            op,
            from: 0.0,
            span: 1.0,
        }
    }
}

/// Operation work found it was to stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("stopped")]
pub struct Stopped;

/// What one operation's work is given of its run's [`Control`]: whether to
/// stop, and where to say how far it has got. By default it watches nothing.
#[derive(Debug, Clone)]
pub struct Watch {
    shared: Option<Arc<Shared>>,
    op: usize,
    /// The share of the operation this watch's work starts at, and how much
    /// of it it is: a part of the work told how far it has got through itself
    /// says how far the operation has.
    from: f64,
    span: f64,
}

impl Default for Watch {
    fn default() -> Self {
        Self {
            shared: None,
            op: 0,
            from: 0.0,
            span: 1.0,
        }
    }
}

impl PartialEq for Watch {
    /// Every watch is the same as far as what is made goes: it decides whether
    /// the work goes on, never what it makes. So parameters holding one compare
    /// by what they make.
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Watch {
    /// Whether the work is to stop.
    pub fn stopped(&self) -> bool {
        self.shared
            .as_ref()
            .is_some_and(|s| s.stop_from.load(Ordering::Relaxed) <= self.op)
    }

    /// `Err(Stopped)` once the work is to stop: for `?` in its loops.
    pub fn check(&self) -> Result<(), Stopped> {
        if self.stopped() { Err(Stopped) } else { Ok(()) }
    }

    /// The work has got `share` of the way through, from 0 to 1. How far the
    /// operation has got only goes up, so a part told out of turn is no step
    /// back.
    pub fn report(&self, share: f64) {
        let Some(shared) = &self.shared else {
            return;
        };
        let Some(done) = shared.done.get(self.op) else {
            return;
        };
        let at = (self.from + self.span * share.clamp(0.0, 1.0)).clamp(0.0, 1.0);
        // Within 0 and the whole, so it fits.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let at = (at * SHARES).round() as u32;
        done.fetch_max(at, Ordering::Relaxed);
    }

    /// Another `share` of this work is done, from 0 to 1, on top of what is: for
    /// work in pieces done at once, each adding its own as it finishes. It
    /// never takes the operation past done.
    pub fn add(&self, share: f64) {
        let Some(done) = self.shared.as_ref().and_then(|s| s.done.get(self.op)) else {
            return;
        };
        // Within the whole, so it fits.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let by = (self.span * share.clamp(0.0, 1.0) * SHARES).round() as u32;
        let _ = done.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |d| {
            Some(d.saturating_add(by).min(WHOLE))
        });
    }

    /// The part of this work from `from` to `to` of the way through it, as a
    /// watch of its own: its work's 0 to 1 is this one's `from` to `to`.
    #[must_use]
    pub fn part(&self, from: f64, to: f64) -> Self {
        let (from, to) = (from.clamp(0.0, 1.0), to.clamp(0.0, 1.0));
        Self {
            shared: self.shared.clone(),
            op: self.op,
            from: self.from + self.span * from,
            span: self.span * (to - from).max(0.0),
        }
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn a_default_watch_never_stops_and_reports_nowhere() {
        let w = Watch::default();
        assert!(!w.stopped());
        w.report(0.5);
        assert_eq!(w.check(), Ok(()));
    }

    #[test]
    fn stopping_from_an_operation_leaves_the_ones_before_going() {
        let c = Control::new(3);
        c.stop_from(1);
        assert!(!c.watch(0).stopped());
        assert!(c.watch(1).stopped() && c.watch(2).stopped());
        // From the earlier of two asked.
        c.stop_from(2);
        assert_eq!(c.stopped_from(), Some(1));
        c.stop();
        assert!(c.watch(0).stopped());
    }

    #[test]
    fn a_part_reports_its_share_of_the_whole_and_never_back() {
        let c = Control::new(1);
        let w = c.watch(0);
        w.part(0.5, 1.0).report(0.5);
        assert_eq!(c.done(0), 0.75);
        w.part(0.0, 0.5).report(1.0);
        assert_eq!(c.done(0), 0.75, "no step back");
        w.report(1.0);
        assert_eq!(c.done(0), 1.0);
    }

    #[test]
    fn a_share_is_done_of_of_and_of_none_is_all() {
        assert_eq!(share(1, 4), 0.25);
        assert_eq!(share(5, 4), 1.0);
        assert_eq!(share(0, 0), 1.0);
    }

    #[test]
    fn pieces_done_at_once_add_up_to_no_more_than_the_whole() {
        let c = Control::new(1);
        let w = c.watch(0).part(0.0, 0.5);
        w.add(0.5);
        w.add(0.5);
        assert_eq!(c.done(0), 0.5);
        c.watch(0).add(0.75);
        assert_eq!(c.done(0), 1.0);
    }
}
