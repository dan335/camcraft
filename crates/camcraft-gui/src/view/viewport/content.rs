//! What the viewport is asked to draw, as plain data: the part, the run, the
//! material and how much of it changed, outlines, height planes, and what is
//! picked; and where the pointer is over the view, as the picker needs it.

use camcraft_geom::mesh::{Aabb, TriMesh};
use camcraft_sim::stock::CellRect;
use eframe::egui;
use glam::{DVec2, DVec3, Vec3};

use super::gpu::{BOTTOM, CLEARANCE, TOP};
use crate::picking::ray::Projector;
use crate::stock::stock_view::{ColorMode, StockImage};
use crate::ui::theme;
use crate::view::scene::Scene;

/// What an outline line shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutlineKind {
    /// An edge of the stock.
    Stock,
    /// The work zero's X axis.
    AxisX,
    /// Its Y axis.
    AxisY,
    /// Its Z axis.
    AxisZ,
    /// A picked edge.
    Selected,
    /// The edge under the pointer.
    Hover,
    /// The machining boundary an operation is kept to, as chosen.
    Boundary,
    /// Where the tool's centre may go against it.
    Limit,
    /// Hatching over what the tool cuts outside the part.
    Channel,
    /// A stock that could be picked into the setup.
    Ghost,
    /// The edge of one of an operation's height planes.
    Plane(PlaneKind),
}

/// Which of an operation's heights a plane shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaneKind {
    /// Where it moves between cuts.
    Clearance,
    /// Where cutting starts.
    Top,
    /// How deep it cuts.
    Bottom,
}

impl PlaneKind {
    /// All of them, in the order their colours are kept.
    pub(super) const ALL: [PlaneKind; 3] =
        [PlaneKind::Clearance, PlaneKind::Top, PlaneKind::Bottom];

    /// Its colour, from the toolpath palette: rapids fly at the clearance, leads
    /// come in at the top, and cuts reach the bottom.
    pub fn color(self) -> [u8; 3] {
        match self {
            PlaneKind::Clearance => theme::PATH_RAPID,
            PlaneKind::Top => theme::PATH_LEAD,
            PlaneKind::Bottom => theme::PATH_CUT,
        }
    }

    pub(super) fn line_kind(self) -> u32 {
        match self {
            PlaneKind::Clearance => CLEARANCE,
            PlaneKind::Top => TOP,
            PlaneKind::Bottom => BOTTOM,
        }
    }

    pub(super) fn index(self) -> usize {
        match self {
            PlaneKind::Clearance => 0,
            PlaneKind::Top => 1,
            PlaneKind::Bottom => 2,
        }
    }
}

/// A level plane across the stock, in the setup's frame, showing one of an
/// operation's heights.
#[derive(Debug, Clone, PartialEq)]
pub struct Plane {
    /// Which height.
    pub kind: PlaneKind,
    /// Its height.
    pub z: f64,
    /// Its corners, seen from above.
    pub lo: DVec2,
    pub hi: DVec2,
    /// What it is called in the view, beside its corner.
    pub label: String,
}

impl Plane {
    /// Its corners, round the rectangle.
    pub(super) fn corners(&self) -> [DVec3; 4] {
        let (lo, hi, z) = (self.lo, self.hi, self.z);
        [
            DVec3::new(lo.x, lo.y, z),
            DVec3::new(hi.x, lo.y, z),
            DVec3::new(hi.x, hi.y, z),
            DVec3::new(lo.x, hi.y, z),
        ]
    }
}

/// A line that is not a move: the stock's edges and the work zero's axes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Outline {
    /// One end.
    pub a: DVec3,
    /// The other.
    pub b: DVec3,
    /// What it shows.
    pub kind: OutlineKind,
}

/// What to show this frame, decided by the controls.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// Draw the part.
    pub part: bool,
    /// Draw the stock's outline.
    pub stock: bool,
    /// Draw the work zero's axes.
    pub origin: bool,
    /// Bit per role, in [`theme::ROLES`] order.
    pub roles: u32,
    /// Bit per operation, from zero. Operations past 32 are always shown.
    pub ops: u32,
    /// Segments shown, in machine order.
    pub shown: usize,
    /// The cutter to draw, as an operation index, and where its tip is.
    pub cutter: Option<(usize, DVec3)>,
}

impl Default for View {
    fn default() -> Self {
        Self {
            part: true,
            stock: false,
            origin: true,
            roles: u32::MAX,
            ops: u32::MAX,
            shown: usize::MAX,
            cutter: None,
        }
    }
}

/// Features to draw over the part, in the setup's frame: what is picked, or what
/// is under the pointer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Highlight {
    /// Faces, as their triangles.
    pub faces: Vec<[Vec3; 3]>,
    /// Edges, and the boundary an operation is kept to.
    pub lines: Vec<Outline>,
    /// Points.
    pub markers: Vec<DVec3>,
}

/// Where the pointer is over the view, as the picker needs it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerEvent {
    /// Through the pointer: where it starts, and a unit direction.
    pub ray: (DVec3, DVec3),
    /// Where the view's lines of sight come from, for what can be seen.
    pub sight: crate::view::camera::Sight,
    /// The pointer, in points.
    pub pos: egui::Pos2,
    /// Whether it clicked this frame: pressed and let go without dragging, so an
    /// orbit never picks.
    pub clicked: bool,
    /// Where the setup's frame is drawn.
    pub projector: Projector,
}

/// What the viewport is asked to draw, with numbers that change whenever the
/// underlying data does, so each piece is uploaded once.
pub struct Content<'a> {
    /// The part mesh.
    pub part: Option<(u64, &'a TriMesh)>,
    /// Changes when a different model is opened, and only then: the view is framed
    /// on it once. Re-running the same model must not snap the camera back.
    pub framing: u64,
    /// The run's toolpaths and cutters.
    pub scene: Option<(u64, &'a Scene)>,
    /// The stock's edges and the work zero's axes.
    pub outline: Option<(u64, &'a [Outline])>,
    /// The box around all the stock: the view frames it with the part, and its
    /// zoom and clipping reach it, however small the part is on it.
    pub stock_box: Option<Aabb>,
    /// The simulated material to draw.
    pub stock: Option<StockShow<'a>>,
    /// What is visible.
    pub view: View,
    /// What is picked.
    pub selection: Option<(u64, &'a Highlight)>,
    /// What is under the pointer.
    pub hover: Option<(u64, &'a Highlight)>,
    /// What could be picked into the setup but is not in it, drawn faintly.
    pub ghost: Option<(u64, &'a Highlight)>,
    /// What to tell the person, over the view: what a click does now.
    pub hint: Option<&'a str>,
    /// An operation's heights, as planes across the stock.
    pub planes: &'a [Plane],
}

/// What changed in the material since it was last uploaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StockChange {
    /// Nothing.
    Nothing,
    /// Any of it.
    All,
    /// These cells.
    Cells(Vec<CellRect>),
}

/// The material's image, playback's, shared with its thread and locked only
/// while it is read.
pub struct StockRef<'a>(pub &'a std::sync::Mutex<StockImage>);

impl StockRef<'_> {
    /// Look at the image, locked for no longer than `f` takes.
    pub fn with<R>(&self, f: impl FnOnce(&StockImage) -> R) -> R {
        f(&self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner))
    }
}

/// Material to draw, and how much of it to upload.
pub struct StockShow<'a> {
    /// Changes whenever the image is a different one.
    pub key: (u64, usize),
    /// The material.
    pub image: StockRef<'a>,
    /// What changed in it since it was last uploaded under the same key.
    pub changed: StockChange,
    /// How to colour it.
    pub mode: ColorMode,
}

impl Content<'_> {
    /// Nothing to draw.
    pub fn empty() -> Self {
        Content {
            part: None,
            framing: 0,
            scene: None,
            outline: None,
            stock_box: None,
            stock: None,
            view: View::default(),
            selection: None,
            hover: None,
            ghost: None,
            hint: None,
            planes: &[],
        }
    }
}
