# 0017 — What a run keeps: every stage, by what it is worked out from

**Status:** decided, 2026-11-04 · **Milestone:** 0.5

## Question

A setup's run kept every operation made before and unchanged since, up to the
first changed one, and worked that one and everything after it out afresh
([decision 0013](0013-generating-by-itself.md)). On a serving tray, a change to
the adaptive roughing worked out the scallop finish after it again too, though
scallop's passes are found from the part alone. What does each stage of a run
depend on, and what can be kept?

## What was decided

A run's work is split by what it is worked out from, and each piece is kept
for as long as that is unchanged:

| Stage | Worked out from | Kept |
| --- | --- | --- |
| The setup laid out (`Prepared`) | the part, its stocks, how it sits | the window's layout, taken by the run (`LaidOut`) |
| The part's index for drops, its surface as the simulator sees it, its flats | the layout; the grid's pitch; the tolerance | on the layout, once each (`Memo`) |
| An operation's passes (`Planned`) | it, its tool, the layout; for rest machining, the operations before it | with its result, and by those inputs in the window (`PassCache`) |
| Its links, and its cut into the material | its passes and the material before it | with its result, while everything before it is kept |
| The material after it | the material before it and its cut | as a keyframe, put back rather than cut again |

- **Passes are linked again** (`Plan::Relink`) for an operation unchanged
  itself after one that changed: its links stay down, lift and ramp over the
  material as it now stands, as they always did, so a run is never planned
  through material it no longer knows about. Only finding the passes is
  saved. Its program is byte for byte the program working them out again
  gives.
- **Kept by what they were worked out from, not by where they are**: the
  operation and its tool, and what shapes the setup's cuts (tolerance, models,
  stocks, orientation, work zero, each model as loaded). An operation moved,
  an edit undone, or an operation turned off and on finds them again. A rest
  machining operation's passes are kept by the operations before it too. A
  setup keeps the passes of each of its operations and eight more.
- **A kept operation's material is put back** from the keyframe its run kept,
  where every operation before it is kept too and the grid is the same;
  otherwise it is cut again, as before.
- **A run takes the window's layout** rather than laying the setup out again,
  where it is laid out as the job has it, and with it what has been worked out
  from the part: the index drops search, built once for all a run's
  strategies and its checks; the part's surface for the simulator, at the
  run's pitch; its flats. `Ran` keeps the layout, shared, not a copy of the
  part.

## Result

On the serving tray (adaptive, then scallop; 440 000 triangles), a full run
took 161 s.

| Change | Before | Now |
| --- | --- | --- |
| Adaptive's stepover, scallop's passes linked again | 180 s | 171 s |
| Scallop's feed, adaptive kept | 21 s | 20 s |
| Scallop's feed changed and changed back (adaptive kept, scallop's passes found again) | 21 s | 6 s |

The first saves little: scallop's passes were already found at once with
adaptive's, on other cores, so finding them again cost mostly what it took from
adaptive. The last is where the stages show: putting adaptive's material back
rather than cutting its 1.3 million moves again took 14 s to 6 s, and linking
scallop's passes again rather than finding them, most of the rest.

## Not taken yet

- Passes kept by only what finding them reads. An operation is compared whole,
  so a change to its feeds or its retract height finds its passes again,
  though feeds could be put on passes kept and a retract height is only the
  linker's.
- Adaptive's levels kept one by one: a change to one setting works out every
  level again.
- Keeping anything between sessions.
