//! A job's sheets arranged: where each part of each of its arranged stocks
//! stands, from the part's body and its sheet's settings.
//!
//! A part is laid by its outline: its body as it stands by its own rotation
//! and scale, seen from above ([`silhouette`]). Casting one is the slow part,
//! and it depends on the body alone, so outlines are kept ([`Outlines`]) by
//! what they are cast from: a sheet's spacing changed lays the same outlines
//! again, and casts none.
//!
//! Each copy of a part is a part to the packer, of its model's one outline,
//! at the turns its part allows: its own grain where it gives one, else the
//! sheet's. A part some of whose copies have no room keeps those that fit.
//!
//! The packing moves parts across the sheet only; the sheet's seat stands
//! each on the sheet's bottom face or up to its top, by the body's own lowest
//! or highest point. Re-arranging changes only where parts stand: no body is
//! placed again, copied or indexed again for it.

use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use camcraft_geom::contour::{Region, RegionError};
use camcraft_geom::mesh::TriMesh;
use camcraft_geom::topology::Body;
use camcraft_geom::turn::Turn;
use glam::{DVec2, DVec3};
use rayon::prelude::*;

use super::{ArrangeError, Direction, Outcome, Part, Sheet, Stand, WHERE_IT_IS, pack};
use crate::control::{Stopped, Watch, share};
use crate::job::{Arranged, Job, MOST_COPIES, Seat, StockSpec};
use crate::must;
use crate::setup::features::silhouette;

/// How many outlines are kept beyond those the latest arrangement used: the
/// parts of a few edits back, for an undo to lay again without casting.
const KEPT: usize = 64;

/// What an outline is cast from, told apart by the triangles themselves: two
/// bodies alike share one outline, and a body changed in any way, placed
/// again, tessellated again or read again from a file since changed, is cast
/// afresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cast {
    /// Every vertex's bits and every triangle's corners, hashed.
    hash: u64,
    vertices: usize,
    triangles: usize,
}

impl Cast {
    fn of(mesh: &TriMesh) -> Self {
        let mut h = DefaultHasher::new();
        for v in mesh.vertices() {
            v.to_array().map(f64::to_bits).hash(&mut h);
        }
        mesh.triangles().hash(&mut h);
        Self {
            hash: h.finish(),
            vertices: mesh.vertices().len(),
            triangles: mesh.len(),
        }
    }
}

/// Bodies' outlines seen from above, kept by what each is cast from, the
/// latest used last. Cheap to copy: each outline is shared.
#[derive(Debug, Clone, Default)]
pub struct Outlines {
    kept: Vec<(Cast, Arc<Region>)>,
}

impl Outlines {
    /// How many are kept.
    pub fn len(&self) -> usize {
        self.kept.len()
    }

    /// Whether none is.
    pub fn is_empty(&self) -> bool {
        self.kept.is_empty()
    }

    /// Take in what `other` keeps, as used latest: for outlines cast on a
    /// copy of these, elsewhere, to be kept here.
    pub fn merge(&mut self, other: Outlines) {
        let theirs = other.kept.len();
        for (cast, outline) in other.kept {
            self.keep(cast, outline);
        }
        self.trim(theirs.max(KEPT));
    }

    /// The outline cast from `cast`, if kept, now used latest.
    fn take(&mut self, cast: &Cast) -> Option<Arc<Region>> {
        let i = self.kept.iter().position(|(c, _)| c == cast)?;
        let entry = self.kept.remove(i);
        let outline = Arc::clone(&entry.1);
        self.kept.push(entry);
        Some(outline)
    }

    /// Keep `outline`, cast from `cast`, as used latest.
    fn keep(&mut self, cast: Cast, outline: Arc<Region>) {
        self.kept.retain(|(c, _)| *c != cast);
        self.kept.push((cast, outline));
    }

    /// Let the least lately used go, beyond the latest `most`.
    fn trim(&mut self, most: usize) {
        if self.kept.len() > most {
            self.kept.drain(..self.kept.len() - most);
        }
    }
}

/// Where every part of a job's sheets stands, and how each sheet was laid.
///
/// It is the one record of where a model on a sheet stands: the bodies are
/// the models', as loaded, and an arrangement only says where their copies
/// go.
#[derive(Debug, Clone, Default)]
pub struct Arrangement {
    /// Where each copy of each model on a sheet laid out stands, by model.
    stands: BTreeMap<String, Vec<Stand>>,
    /// Each of the job's arranged stocks, in the job's order.
    sheets: Vec<SheetLayout>,
}

/// How one sheet was laid.
#[derive(Debug, Clone)]
pub struct SheetLayout {
    /// The stock, by name.
    pub stock: String,
    /// The models it arranges, in its order.
    pub models: Vec<String>,
    /// Whether it was, and if so how.
    pub laid: Laid,
}

/// Whether a sheet was laid, and how.
#[derive(Debug, Clone)]
pub enum Laid {
    /// Not yet: the bodies of these of its parts were not given, as while
    /// they are loaded.
    Waiting(Vec<String>),
    /// It could not be, and why.
    Failed(ArrangeError),
    /// Every part, in the sheet's order, and how far along the sheet they
    /// reach.
    Done {
        /// Each part.
        parts: Vec<PartLaid>,
        /// How far along the sheet's length the parts placed reach, from the
        /// edge packing starts at, in millimetres: none for none placed.
        used: f64,
        /// The sheet's length along the axis it is used up along.
        length: f64,
    },
}

/// One part of a sheet, laid: each of its copies.
#[derive(Debug, Clone)]
pub struct PartLaid {
    /// The model, by name.
    pub model: String,
    /// Each copy, in order: where it stands, or why it is left out. Copies
    /// are laid in their order, so any left out are the last.
    pub copies: Vec<CopyLaid>,
    /// How low and how high each copy reaches, seated on the sheet: where it
    /// stands, or would stand were there room for it. A turn about Z moves
    /// neither, so every copy reaches the same.
    pub heights: [f64; 2],
}

/// One copy of a part, laid.
#[derive(Debug, Clone)]
pub struct CopyLaid {
    /// Where it stands, turned and moved and seated; or why it is left out.
    pub outcome: Outcome,
    /// Its outline seen from above, where it stands; none for a copy left
    /// out.
    pub outline: Option<Region>,
}

impl PartLaid {
    /// Where each copy placed stands, in copy order.
    pub fn stands(&self) -> impl Iterator<Item = Stand> + '_ {
        self.copies.iter().filter_map(|c| match c.outcome {
            Outcome::Placed(stand) => Some(stand),
            Outcome::TooLarge | Outcome::NoRoom => None,
        })
    }

    /// How many of its copies were placed.
    pub fn placed(&self) -> usize {
        self.stands().count()
    }

    /// How many of its copies were left out as too large for the sheet at any
    /// turn they may take.
    pub fn too_large(&self) -> usize {
        self.copies
            .iter()
            .filter(|c| c.outcome == Outcome::TooLarge)
            .count()
    }

    /// How many of its copies were left out for want of room among the
    /// others.
    pub fn no_room(&self) -> usize {
        self.copies
            .iter()
            .filter(|c| c.outcome == Outcome::NoRoom)
            .count()
    }
}

impl Arrangement {
    /// The arrangement of a job with no sheets: every model stands once,
    /// where it is.
    pub fn none() -> &'static Self {
        static NONE: Arrangement = Arrangement {
            stands: BTreeMap::new(),
            sheets: Vec::new(),
        };
        &NONE
    }

    /// Each sheet, in the job's order.
    pub fn sheets(&self) -> &[SheetLayout] {
        &self.sheets
    }

    /// Sheet `stock`, if the job had one so named.
    pub fn sheet(&self, stock: &str) -> Option<&SheetLayout> {
        self.sheets.iter().find(|s| s.stock == stock)
    }

    /// Where each copy of model `name` stands: each copy its sheet placed, in
    /// copy order, none of those it had no room for; once, where it is, for a
    /// model no
    /// sheet arranges. `None` for one on a sheet that was not laid, waiting
    /// for a body or failed, which stands nowhere yet.
    pub fn stands(&self, name: &str) -> Option<&[Stand]> {
        if let Some(stands) = self.stands.get(name) {
            return Some(stands);
        }
        let on_a_sheet = self
            .sheets
            .iter()
            .any(|s| s.models.iter().any(|m| m == name));
        (!on_a_sheet).then_some(WHERE_IT_IS)
    }
}

impl SheetLayout {
    /// Each part, where the sheet was laid; none where it was not.
    pub fn parts(&self) -> &[PartLaid] {
        match &self.laid {
            Laid::Done { parts, .. } => parts,
            Laid::Waiting(_) | Laid::Failed(_) => &[],
        }
    }

    /// Each part some copy of which is left out, where the sheet was laid:
    /// its copies say which and why.
    pub fn left_out(&self) -> impl Iterator<Item = &PartLaid> {
        self.parts().iter().filter(|p| p.placed() < p.copies.len())
    }

    /// How many copies it placed, of how many its parts ask for, where it was
    /// laid.
    pub fn placed(&self) -> (usize, usize) {
        self.parts().iter().fold((0, 0), |(placed, all), p| {
            (placed + p.placed(), all + p.copies.len())
        })
    }
}

/// Every arranged stock of `job` laid, with the bodies `bodies` as loaded,
/// by model, each where its own rotation and scale place it, with no
/// position of its own. A sheet some of whose parts' bodies are not given
/// is not laid, and says which it waits for; one that cannot be laid says
/// why. The same job and bodies lay out the same, on any number of threads.
///
/// Outlines are cast only for bodies `outlines` does not keep, and kept
/// there. `watch` is told how far it has got, and stops it: only stopping
/// stops it short of every sheet.
pub fn arrange(
    job: &Job,
    bodies: &BTreeMap<&str, &Body>,
    outlines: &mut Outlines,
    watch: &Watch,
) -> Result<Arrangement, Stopped> {
    let sheets: Vec<(&str, &Arranged)> = job
        .stocks
        .iter()
        .filter_map(|s| match &s.spec {
            StockSpec::Arranged(a) => Some((s.name.as_str(), a)),
            StockSpec::AroundModels { .. } | StockSpec::FixedBox(_) => None,
        })
        .collect();
    let mut out = Arrangement::default();
    let mut used = 0;
    for (k, &(stock, a)) in sheets.iter().enumerate() {
        let part = watch.part(share(k, sheets.len()), share(k + 1, sheets.len()));
        let laid = match lay(a, bodies, outlines, &part) {
            Ok(laid) => laid,
            Err(ArrangeError::Stopped(stopped)) => return Err(stopped),
            Err(e) => Laid::Failed(e),
        };
        if let Laid::Done { parts, .. } = &laid {
            // One outline for each part, however many copies of it.
            used += parts.len();
            for p in parts {
                out.stands.insert(p.model.clone(), p.stands().collect());
            }
        }
        out.sheets.push(SheetLayout {
            stock: stock.to_string(),
            models: a.parts.iter().map(|p| p.model.clone()).collect(),
            laid,
        });
    }
    // What this arrangement used, however many, and a few more besides.
    outlines.trim(KEPT + used);
    watch.report(1.0);
    Ok(out)
}

/// Sheet `a` laid with `bodies`, telling `watch`.
fn lay(
    a: &Arranged,
    bodies: &BTreeMap<&str, &Body>,
    outlines: &mut Outlines,
    watch: &Watch,
) -> Result<Laid, ArrangeError> {
    let mut given: Vec<&Body> = Vec::with_capacity(a.parts.len());
    let mut missing: Vec<String> = Vec::new();
    for p in &a.parts {
        match bodies.get(p.model.as_str()) {
            Some(body) => given.push(body),
            None => missing.push(p.model.clone()),
        }
    }
    if !missing.is_empty() {
        return Ok(Laid::Waiting(missing));
    }
    let sheet_turns = a.rotation.turns().ok_or(ArrangeError::Sheet {
        what: "step between turns",
        must: "from 1 to 180 degrees",
    })?;
    // Each part's turns: its grain where it has one, the sheet's where not.
    let mut turns: Vec<Vec<Turn>> = Vec::with_capacity(a.parts.len());
    for p in &a.parts {
        if !(1..=MOST_COPIES).contains(&p.copies) {
            return Err(ArrangeError::Copies {
                model: p.model.clone(),
                copies: p.copies,
            });
        }
        turns.push(match p.rotation {
            Some(grain) => grain
                .turns()
                .ok_or_else(|| ArrangeError::Grain(p.model.clone()))?,
            None => sheet_turns.clone(),
        });
    }
    let shapes = outlines_of(&given, outlines, &watch.part(0.0, 0.2))?;
    for (p, shape) in a.parts.iter().zip(&shapes) {
        if shape.area() <= 0.0 {
            return Err(ArrangeError::NoOutline(p.model.clone()));
        }
    }
    let [x, y, z] = a.corner;
    let [length, width, thickness] = a.size;
    if !crate::is_positive(thickness) {
        return Err(ArrangeError::Sheet {
            what: "thickness",
            must: must::POSITIVE,
        });
    }
    if !z.is_finite() {
        return Err(ArrangeError::Sheet {
            what: "corner",
            must: must::FINITE,
        });
    }
    let sheet = Sheet {
        corner: DVec2::new(x, y),
        size: DVec2::new(length, width),
        spacing: a.spacing,
        border: a.border,
        from: a.from,
        along: a.along,
    };
    // Each copy is a part to the packer, of its model's outline and turns,
    // the copies of a part one after another in their order.
    let packed: Vec<Part> = turns
        .iter()
        .zip(&a.parts)
        .enumerate()
        .flat_map(|(shape, (turns, p))| {
            (0..p.copies).map(move |_| Part {
                shape,
                turns: turns.clone(),
            })
        })
        .collect();
    let mut outcomes = pack(&sheet, &shapes, &packed, &watch.part(0.2, 1.0))?.into_iter();

    // Seated on the sheet by the body's own lowest or highest point: a turn
    // about Z moves neither.
    let (bottom, top) = (z, z + thickness);
    let mut laid = Vec::with_capacity(a.parts.len());
    for ((p, body), shape) in a.parts.iter().zip(&given).zip(&shapes) {
        let b = body
            .mesh()
            .bounds()
            .ok_or_else(|| ArrangeError::NoOutline(p.model.clone()))?;
        let rise = match a.seat {
            Seat::Bottom => bottom - b.min.z,
            Seat::Top => top - b.max.z,
        };
        let copies = outcomes
            .by_ref()
            .take(usize::try_from(p.copies).unwrap_or(0))
            .map(|outcome| match outcome {
                Outcome::Placed(stand) => {
                    let stand = Stand {
                        turn: stand.turn,
                        offset: DVec3::new(stand.offset.x, stand.offset.y, rise),
                    };
                    CopyLaid {
                        outcome: Outcome::Placed(stand),
                        outline: Some(stand.region(shape)),
                    }
                }
                left_out => CopyLaid {
                    outcome: left_out,
                    outline: None,
                },
            })
            .collect();
        laid.push(PartLaid {
            model: p.model.clone(),
            copies,
            heights: [b.min.z + rise, b.max.z + rise],
        });
    }
    let used = used(
        &sheet,
        laid.iter()
            .flat_map(|p| &p.copies)
            .filter_map(|c| c.outline.as_ref()),
    );
    let length = match sheet.along {
        Direction::X => length,
        Direction::Y => width,
    };
    Ok(Laid::Done {
        parts: laid,
        used,
        length,
    })
}

/// The outline of each of `bodies` seen from above, in order: kept in
/// `outlines`, or cast, each once however many share it, and kept there.
fn outlines_of(
    bodies: &[&Body],
    outlines: &mut Outlines,
    watch: &Watch,
) -> Result<Vec<Region>, ArrangeError> {
    let casts: Vec<Cast> = bodies.par_iter().map(|b| Cast::of(b.mesh())).collect();
    watch.check()?;
    let mut found: Vec<Option<Arc<Region>>> = casts.iter().map(|c| outlines.take(c)).collect();
    // Those kept nowhere, each once.
    let mut wanted: Vec<(Cast, &Body)> = Vec::new();
    for ((cast, body), had) in casts.iter().zip(bodies).zip(&found) {
        if had.is_none() && !wanted.iter().any(|(c, _)| c == cast) {
            wanted.push((*cast, body));
        }
    }
    let cast: Vec<Result<Region, RegionError>> = wanted
        .par_iter()
        .map(|(_, body)| silhouette(body.mesh()))
        .collect();
    watch.check()?;
    for ((c, _), outline) in wanted.iter().zip(cast) {
        let outline = Arc::new(outline?);
        for (slot, _) in found.iter_mut().zip(&casts).filter(|(_, k)| *k == c) {
            *slot = Some(Arc::clone(&outline));
        }
        outlines.keep(*c, outline);
    }
    watch.report(1.0);
    Ok(found
        .into_iter()
        .map(|o| o.map(|o| (*o).clone()).unwrap_or_default())
        .collect())
}

/// How far along `sheet`'s length the outlines `placed` reach, by their
/// boxes, from the edge packing starts at.
fn used<'a>(sheet: &Sheet, placed: impl Iterator<Item = &'a Region>) -> f64 {
    let (lo, hi) = (sheet.corner, sheet.corner + sheet.size);
    placed
        .filter_map(Region::bounds)
        .map(|(a, b)| match sheet.along {
            Direction::X if sheet.from.left() => b.x - lo.x,
            Direction::X => hi.x - a.x,
            Direction::Y if sheet.from.front() => b.y - lo.y,
            Direction::Y => hi.y - a.y,
        })
        .fold(0.0, f64::max)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests;
