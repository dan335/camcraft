//! A job's sheets arranged: where each part of each of its arranged stocks
//! stands, from the part's body and its sheet's settings.
//!
//! A part is laid by its outline: its body as it stands by its own rotation
//! and scale, seen from above ([`silhouette`]). Casting one is the slow part,
//! and it depends on the body alone, so outlines are kept ([`Outlines`]) by
//! what they are cast from: a sheet's spacing changed lays the same outlines
//! again, and casts none.
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
use glam::{DVec2, DVec3};
use rayon::prelude::*;

use super::{ArrangeError, Direction, Outcome, Part, Sheet, Stand, WHERE_IT_IS, pack};
use crate::control::{Stopped, Watch, share};
use crate::job::{Arranged, Job, Seat, StockSpec};
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

/// One part of a sheet, laid.
#[derive(Debug, Clone)]
pub struct PartLaid {
    /// The model, by name.
    pub model: String,
    /// Where it stands, turned and moved and seated; or why it is left out.
    pub outcome: Outcome,
    /// Its outline seen from above, where it stands; none for a part left
    /// out.
    pub outline: Option<Region>,
    /// How low and how high it reaches, seated on the sheet: where it stands,
    /// or would stand were there room for it.
    pub heights: [f64; 2],
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

    /// Where each copy of model `name` stands: as its sheet laid it, none
    /// where the sheet had no room for it; once, where it is, for a model no
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
    /// Each part left out, with why, where the sheet was laid.
    pub fn left_out(&self) -> impl Iterator<Item = &PartLaid> {
        let parts = match &self.laid {
            Laid::Done { parts, .. } => parts.as_slice(),
            Laid::Waiting(_) | Laid::Failed(_) => &[],
        };
        parts
            .iter()
            .filter(|p| !matches!(p.outcome, Outcome::Placed(_)))
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
            used += parts.len();
            for p in parts {
                let stands = match p.outcome {
                    Outcome::Placed(stand) => vec![stand],
                    Outcome::TooLarge | Outcome::NoRoom => Vec::new(),
                };
                out.stands.insert(p.model.clone(), stands);
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
    let turns = a.rotation.turns().ok_or(ArrangeError::Sheet {
        what: "step between turns",
        must: "from 1 to 180 degrees",
    })?;
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
    let parts: Vec<Part> = (0..shapes.len())
        .map(|shape| Part {
            shape,
            turns: turns.clone(),
        })
        .collect();
    let outcomes = pack(&sheet, &shapes, &parts, &watch.part(0.2, 1.0))?;

    // Seated on the sheet by the body's own lowest or highest point: a turn
    // about Z moves neither.
    let (bottom, top) = (z, z + thickness);
    let mut laid = Vec::with_capacity(outcomes.len());
    for (((p, body), shape), outcome) in a.parts.iter().zip(&given).zip(&shapes).zip(outcomes) {
        let b = body
            .mesh()
            .bounds()
            .ok_or_else(|| ArrangeError::NoOutline(p.model.clone()))?;
        let rise = match a.seat {
            Seat::Bottom => bottom - b.min.z,
            Seat::Top => top - b.max.z,
        };
        let (outcome, outline) = match outcome {
            Outcome::Placed(stand) => {
                let stand = Stand {
                    turn: stand.turn,
                    offset: DVec3::new(stand.offset.x, stand.offset.y, rise),
                };
                (Outcome::Placed(stand), Some(stand.region(shape)))
            }
            left_out => (left_out, None),
        };
        laid.push(PartLaid {
            model: p.model.clone(),
            outcome,
            outline,
            heights: [b.min.z + rise, b.max.z + rise],
        });
    }
    let used = used(&sheet, laid.iter().filter_map(|p| p.outline.as_ref()));
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
