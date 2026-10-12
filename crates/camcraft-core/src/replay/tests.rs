use super::*;
use crate::fixture::JOB;
use crate::run::run_setup;
use crate::setup::Bodies;
use camcraft_geom::synthetic;
use camcraft_geom::topology::Body;

fn run() -> (Job, SetupResult) {
    let job: Job = toml::from_str(JOB).unwrap();
    let dome = Body::derived(synthetic::uv_sphere(8.0, 60, 84));
    let bodies: Bodies<'_> = [("dome", &dome)].into_iter().collect();
    let result = run_setup(&job, 0, &bodies).unwrap();
    (job, result)
}

/// The run's keyframes, unpacked.
fn stages(result: &SetupResult) -> Vec<Stock> {
    result
        .keyframes
        .iter()
        .map(|s| s.unpack().unwrap())
        .collect()
}

fn bits(stock: &Stock) -> Vec<u32> {
    stock.field().data().iter().map(|v| v.to_bits()).collect()
}

/// An operation turned off may have no tool; the rest still replay.
#[test]
fn an_operation_turned_off_without_a_tool_still_replays() {
    let mut job: Job = toml::from_str(JOB).unwrap();
    let rough = &mut job.setups[0].operations[0];
    rough.enabled = false;
    rough.tool = None;
    let dome = Body::derived(synthetic::uv_sphere(8.0, 60, 84));
    let bodies: Bodies<'_> = [("dome", &dome)].into_iter().collect();
    let result = run_setup(&job, 0, &bodies).unwrap();
    let replay = Replay::new(&job, &job.setups[0], &result).unwrap();
    assert_eq!(replay.moves_in(0), 0);
    assert!(replay.moves_in(1) > 0);
}

/// The keyframes are what the run passed through: the untouched block first, the
/// finished stock last, and material only ever removed in between.
#[test]
fn stages_are_the_keyframes_of_the_run() {
    let (job, result) = run();
    assert_eq!(result.keyframes.len(), job.setups[0].operations.len() + 1);
    let block = &stages(&result)[0];
    let top = result.heights.stock_top;
    assert!(
        block
            .field()
            .data()
            .iter()
            .all(|h| *h == camcraft_geom::heightmap::narrow(top))
    );
    for pair in stages(&result).windows(2) {
        let [a, b] = pair else { continue };
        for (before, after) in a.field().data().iter().zip(b.field().data()) {
            assert!(after <= before, "material appeared: {before} -> {after}");
        }
    }
}

/// Replaying each operation from its keyframe lands exactly on the next one.
#[test]
fn replay_reproduces_every_stage() {
    let (job, result) = run();
    let replay = Replay::new(&job, &job.setups[0], &result).unwrap();
    for op in 0..job.setups[0].operations.len() {
        let n = replay.moves_in(op);
        let got = replay.stock_at(Position { op, moves: n }).unwrap();
        assert_eq!(
            bits(&got),
            bits(&stages(&result)[op + 1]),
            "operation {op} replayed differently from how it ran"
        );
    }
    assert!(replay.stock_at(Position { op: 9, moves: 0 }).is_none());
}

/// The property playback rests on: sweeping in pieces of any size equals
/// sweeping whole, bit for bit.
#[test]
fn pieces_equal_the_whole() {
    let (job, result) = run();
    let replay = Replay::new(&job, &job.setups[0], &result).unwrap();
    // A fixed pseudo-random sequence: reproducible, and no dependency.
    let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = |max: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        1 + usize::try_from(seed % max as u64).unwrap()
    };
    for op in 0..job.setups[0].operations.len() {
        let n = replay.moves_in(op);
        assert!(n > 50, "operation {op} is too short to prove anything");
        let whole = replay.stock_at(Position { op, moves: n }).unwrap();

        let mut pieces = stages(&result)[op].clone();
        let mut at = 0;
        let mut chunks = 0;
        while at < n {
            // Pieces of up to a twentieth of the operation: always many.
            let to = (at + next((n / 20).max(2))).min(n);
            replay.advance(&mut pieces, op, at, to);
            at = to;
            chunks += 1;
        }
        assert!(chunks > 10);
        assert_eq!(
            bits(&pieces),
            bits(&whole),
            "operation {op} drifted in pieces"
        );

        // And one move at a time, the finest a step can be.
        let mut single = stages(&result)[op].clone();
        for k in 0..n {
            replay.advance(&mut single, op, k, k + 1);
        }
        assert_eq!(
            bits(&single),
            bits(&whole),
            "operation {op} drifted move by move"
        );
    }
}

/// The end of an operation is its keyframe, restored at once however far
/// away: going to the end of the program sweeps nothing on the way.
#[test]
fn the_end_of_an_operation_is_reached_at_once() {
    let (job, result) = run();
    let replay = Replay::new(&job, &job.setups[0], &result).unwrap();
    let mut live = Live::new(&replay, usize::MAX).unwrap();
    for op in (0..replay.ops()).rev().chain(0..replay.ops()) {
        let end = Position {
            op,
            moves: replay.moves_in(op),
        };
        let (_, reached) = live.seek(&replay, end, 0.0, 1);
        assert!(reached, "operation {op}'s end took sweeping");
        assert_eq!(live.at(), end);
        assert_eq!(bits(live.stock()), bits(&stages(&result)[op + 1]));
    }
    // And on from there by sweeping, as before.
    let (_, reached) = live.seek(&replay, Position { op: 0, moves: 3 }, 0.0, usize::MAX);
    assert!(reached);
    assert_eq!(
        bits(live.stock()),
        bits(&replay.stock_at(Position { op: 0, moves: 3 }).unwrap())
    );
}

/// Part-way through an operation the stock lies between its keyframes.
#[test]
fn midway_is_between_the_keyframes() {
    let (job, result) = run();
    let replay = Replay::new(&job, &job.setups[0], &result).unwrap();
    let n = replay.moves_in(0);
    let mid = replay
        .stock_at(Position {
            op: 0,
            moves: n / 2,
        })
        .unwrap();
    let (before, after) = (&stages(&result)[0], &stages(&result)[1]);
    let volume = |s: &Stock| s.volume();
    assert!(
        volume(&mid) < volume(before),
        "half the roughing removed nothing"
    );
    assert!(
        volume(&mid) > volume(after),
        "half the roughing removed everything"
    );
}

/// Cells that differ between two stocks lie inside what a step said changed.
fn assert_reported(before: &Stock, after: &Stock, changed: Changed) {
    let (a, b) = (before.field(), after.field());
    for iy in 0..a.ny() {
        for ix in 0..a.nx() {
            if a.get(ix, iy).map(f64::to_bits) != b.get(ix, iy).map(f64::to_bits) {
                let inside = match changed {
                    Changed::All => true,
                    Changed::Cells(r) => r.contains(ix, iy),
                    Changed::Nothing => false,
                };
                assert!(inside, "cell ({ix}, {iy}) changed outside {changed:?}");
            }
        }
    }
}

/// Seek to completion, checking every step reports what it changed.
fn seek(live: &mut Live, replay: &Replay, to: Position, fraction: f64) {
    loop {
        let before = live.stock().clone();
        let (changed, reached) = live.seek(replay, to, fraction, 400);
        assert_reported(&before, live.stock(), changed);
        if reached {
            return;
        }
    }
}

/// However playback wanders, forward, back, part-way along moves and across
/// operations, a whole position is bit for bit what the replay says it is.
#[test]
fn live_stock_is_the_replay_wherever_it_has_been() {
    let (job, result) = run();
    let replay = Replay::new(&job, &job.setups[0], &result).unwrap();
    // A small budget, room for a few checkpoints the size of the stock after
    // roughing: some to use, and some not kept.
    let budget = result.keyframes[1].bytes() * 4;
    let mut live = Live::new(&replay, budget).unwrap();
    let (n0, n1) = (replay.moves_in(0), replay.moves_in(1));
    let walk = [
        (
            Position {
                op: 0,
                moves: n0 / 3,
            },
            0.0,
        ),
        (
            Position {
                op: 0,
                moves: n0 / 3 + 1,
            },
            0.4,
        ),
        (
            Position {
                op: 1,
                moves: n1 / 2,
            },
            0.7,
        ),
        (
            Position {
                op: 0,
                moves: n0 / 5,
            },
            0.0,
        ),
        (Position { op: 0, moves: n0 }, 0.0),
        (Position { op: 1, moves: 1 }, 0.5),
        (Position { op: 1, moves: n1 }, 0.0),
        (
            Position {
                op: 1,
                moves: n1 / 4,
            },
            0.9,
        ),
        (Position { op: 0, moves: 0 }, 0.0),
        (Position { op: 7, moves: 0 }, 0.0),
    ];
    for (to, fraction) in walk {
        seek(&mut live, &replay, to, fraction);
        if fraction == 0.0 {
            let want = replay
                .stock_at(Position {
                    op: to.op.min(1),
                    moves: if to.op > 1 { n1 } else { to.moves },
                })
                .unwrap();
            assert_eq!(bits(live.stock()), bits(&want), "at {to:?}");
        } else {
            // Part-way lies between the whole moves either side of it, within
            // what sampling the move at other points can differ by.
            let lo = replay.stock_at(to).unwrap();
            let hi = replay
                .stock_at(Position {
                    moves: to.moves + 1,
                    ..to
                })
                .unwrap();
            for ((l, h), v) in lo
                .field()
                .data()
                .iter()
                .zip(hi.field().data())
                .zip(live.stock().field().data())
            {
                assert!(
                    v <= l && v + 0.01 >= *h,
                    "{v} not within [{h}, {l}] at {to:?}"
                );
            }
        }
    }
    assert!(
        live.checkpoints() > 2,
        "the walk should have dropped checkpoints"
    );
    assert!(live.held() <= budget, "over the budget: {}", live.held());
}

/// A long seek comes back in pieces, and a part-way sweep is undone exactly.
#[test]
fn a_seek_goes_in_steps_and_undoes_its_part_way_sweep() {
    let (job, result) = run();
    let replay = Replay::new(&job, &job.setups[0], &result).unwrap();
    let mut live = Live::new(&replay, usize::MAX).unwrap();
    // Somewhere mid-cut, so a part-way move cuts something.
    let moves = result.operations[0].path.moves();
    let made = (300..moves.len())
        .find(|&k| {
            moves[k].role == crate::path::toolpath::MoveRole::Cut
                && moves[k - 1].role == crate::path::toolpath::MoveRole::Cut
        })
        .unwrap();
    let to = Position { op: 0, moves: made };
    let mut steps = 0;
    while !live.seek(&replay, to, 0.0, 50).1 {
        steps += 1;
    }
    assert!(
        steps >= 5,
        "{made} moves in steps of 50 took {steps} extra steps"
    );
    let whole = live.stock().clone();
    seek(&mut live, &replay, to, 0.6);
    assert_ne!(bits(live.stock()), bits(&whole), "part-way cut nothing");
    seek(&mut live, &replay, to, 0.0);
    assert_eq!(bits(live.stock()), bits(&whole));
}

/// Checkpoints swept ahead by a second stock serve the first: seeking back
/// restores one, and still lands on the replay's own bits.
#[test]
fn checkpoints_swept_ahead_are_adopted() {
    let (job, result) = run();
    let replay = Replay::new(&job, &job.setups[0], &result).unwrap();
    let mut shown = Live::new(&replay, usize::MAX).unwrap();
    let mut ahead = Live::new(&replay, usize::MAX).unwrap();
    while !ahead.sweep_ahead(&replay, 100) {}
    let swept = ahead.checkpoints();
    assert!(swept > 2);
    shown.adopt_checkpoints(&mut ahead);
    assert_eq!(shown.checkpoints(), swept);
    assert_eq!(ahead.checkpoints(), 0);
    let to = Position {
        op: 0,
        moves: replay.moves_in(0) * 3 / 4,
    };
    seek(&mut shown, &replay, to, 0.0);
    assert_eq!(bits(shown.stock()), bits(&replay.stock_at(to).unwrap()));
}
