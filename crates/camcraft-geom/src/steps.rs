//! Bounded step counts.
//!
//! A count worked out from a length and a step, such as samples along a move or
//! passes across a region, is bounded rather than trusted: a tiny step against a
//! large length, or a coordinate that is not finite, must not turn one loop into
//! an unbounded one or one allocation into an enormous one.

/// Most steps any one count may take. Staying inside `u32` also keeps every later
/// conversion of a step number to `f64` exact.
pub const MAX_STEPS: u32 = 1_000_000;

/// `n` as a count of steps: at least one, at most [`MAX_STEPS`], and one when it
/// is not finite.
pub fn bounded_count(n: f64) -> u32 {
    if !n.is_finite() || n < 1.0 {
        return 1;
    }
    // Clamped and finite, so the narrowing is exact.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let n = n.min(f64::from(MAX_STEPS)) as u32;
    n
}
