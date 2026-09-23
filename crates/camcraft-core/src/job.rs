//! The job model: models, stocks and setups, each setup an ordered list of
//! operations sharing one stock.
//!
//! This is the contract between a person and the engine, and it is shaped the way
//! Fusion shapes a CAM document. A job keeps three groups:
//!
//! - **Models**: the design files it machines, referenced by path relative to the
//!   job file, never copied in.
//! - **Stocks**: blocks of material, in model coordinates. A block is a physical
//!   thing, so a top and a bottom setup share one and each sees it from its side.
//! - **Setups**: how the part sits on the machine and where its work zero is, which
//!   models and stocks it includes (by name), its post and work offset, and its
//!   operations.
//!
//! An operation is shaped the way Fusion shapes one, **Geometry, Heights, Passes,
//! Linking**, because that factoring is proven and maps one-to-one onto the
//! provisions this project was built around. Adopting it means the parameter layer
//! never needs restructuring as strategies are added.
//!
//! # Heights are symbolic
//!
//! `clearance = { ref = "stock-top", offset = 10 }` rather than a bare number. A job
//! written against one block of material then works against a taller one, and the
//! meaning survives being read six months later. Absolute numbers are still
//! available, but they have to be asked for.
//!
//! # One stock per setup, threaded
//!
//! A setup's operations run in order against a single block. Each reads the
//! material as it currently stands and writes back what it removed, so a finishing
//! pass never re-cuts what roughing already cleared. Rest machining is not a
//! feature here; it is the consequence of that ordering.
//!
//! Each setup starts from its own stock. The material is a height field seen from
//! above, so what a top setup leaves cannot be turned over for the bottom one to
//! start from. Carrying stock between setups needs a volumetric model.
//!
//! # Picked features
//!
//! A work zero can sit on a point picked on a model, and an operation can be kept
//! to faces or edges picked on one ([`FeatureRef`], [`PointRef`]). A pick is saved
//! as the model's name, the feature's number, and a point on it in model
//! coordinates. The point is what the pick *is*: the number is only a quick way to
//! find it while the model is unchanged. A model tessellated again, or re-exported
//! with its faces in another order, still has a face through that point, so the
//! pick survives; a model edited so the point is on nothing any more says so, and
//! nothing is cut until the pick is made again. See `crate::features`.
//!
//! # Versions
//!
//! A build that predates picked features would read a job that uses them, skip the
//! fields it does not know, and machine the whole part where a person had asked for
//! one face. So a job that uses them is written as version 2, which such a build
//! refuses by name, and every other job is still written as version 1, which it
//! reads as before ([`Job::required_version`]).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::frame::{BadOrientation, Orientation};
use crate::tool::{Ball, Bullnose, BullnoseFamily, Flat, ToolError};

/// The newest job file format this build reads and writes. It reads every version
/// from 1, and writes the oldest that holds what a job says
/// ([`Job::required_version`]).
pub const JOB_VERSION: u32 = 2;

/// A complete job.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Job {
    /// The file format; [`JOB_VERSION`].
    pub version: u32,
    /// Chordal tolerance in millimetres. Drives sampling and grid resolution alike,
    /// and how finely STEP models are tessellated.
    #[serde(default = "default_tolerance")]
    pub tolerance: f64,
    /// The job's own cutters, by name: copies, never links to a library, so the job
    /// still cuts what was verified after the library changes.
    #[serde(default)]
    pub tools: BTreeMap<String, ToolSpec>,
    /// The design files machined.
    #[serde(default, rename = "model")]
    pub models: Vec<ModelDef>,
    /// Blocks of material.
    #[serde(default, rename = "stock")]
    pub stocks: Vec<StockDef>,
    /// Setups, in the order they are machined.
    #[serde(default, rename = "setup")]
    pub setups: Vec<Setup>,
}

impl Default for Job {
    /// An empty job at the current format.
    fn default() -> Self {
        Self {
            version: JOB_VERSION,
            tolerance: 0.05,
            tools: BTreeMap::new(),
            models: Vec::new(),
            stocks: Vec::new(),
            setups: Vec::new(),
        }
    }
}

fn default_tolerance() -> f64 {
    0.01
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

/// Units for a file that does not say, as an STL does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LengthUnit {
    /// Millimetres.
    Mm,
    /// Inches.
    Inch,
}

/// One design file.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ModelDef {
    /// Name, unique in the job; setups and stocks refer to it by this.
    pub name: String,
    /// The file, relative to the job file's folder, with `/` between folders. An
    /// absolute path is allowed.
    pub path: String,
    /// What an STL's numbers mean. Required for STL, which does not say; never
    /// guessed. STEP carries its own units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units: Option<LengthUnit>,
}

/// One block of material.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct StockDef {
    /// Name, unique in the job; setups refer to it by this.
    pub name: String,
    /// Its shape.
    #[serde(flatten)]
    pub spec: StockSpec,
}

/// The shape of a block, in model coordinates, millimetres.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum StockSpec {
    /// A box around models, grown by a margin.
    AroundModels {
        /// The models it is around, by name. Empty means the models of whichever
        /// setup uses it.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        models: Vec<String>,
        /// Extra material each side in X and Y, and on top in Z.
        #[serde(default)]
        margin: [f64; 3],
        /// Extra material under the models.
        ///
        /// Zero by default: for a part cut from the top only, material below the
        /// model is the part of the block the cutter never reaches.
        #[serde(default, skip_serializing_if = "is_default")]
        below: f64,
    },
    /// A box at a fixed place and size.
    FixedBox {
        /// Its lowest corner.
        corner: [f64; 3],
        /// Its size along X, Y and Z.
        size: [f64; 3],
    },
}

impl Default for StockSpec {
    /// Around the setup's models, as the block a new job starts with.
    fn default() -> Self {
        StockSpec::AroundModels {
            models: Vec::new(),
            margin: [2.0, 2.0, 2.0],
            below: 0.0,
        }
    }
}

/// How a part is held and machined from one side.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Setup {
    /// Name, unique in the job. Programs are named after it.
    pub name: String,
    /// The job's models this setup machines, by name.
    #[serde(default)]
    pub models: Vec<String>,
    /// The job's stocks this setup starts from, by name.
    #[serde(default)]
    pub stocks: Vec<String>,
    /// Work offset, 54 for `G54`.
    #[serde(default = "default_wcs")]
    pub wcs: u8,
    /// The post processor: a built-in name, or a profile file relative to the job.
    #[serde(default = "default_post")]
    pub post: String,
    /// Which model directions become the machine's Z and X.
    #[serde(default, skip_serializing_if = "is_default")]
    pub orientation: Orientation,
    /// Where the work zero is.
    #[serde(default, skip_serializing_if = "is_default")]
    pub origin: Origin,
    /// Operations, in the order they run.
    #[serde(default, rename = "operation")]
    pub operations: Vec<Operation>,
}

impl Setup {
    /// An empty setup with the usual defaults.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            models: Vec::new(),
            stocks: Vec::new(),
            wcs: default_wcs(),
            post: default_post(),
            orientation: Orientation::default(),
            origin: Origin::default(),
            operations: Vec::new(),
        }
    }
}

fn default_wcs() -> u8 {
    54
}

fn default_post() -> String {
    "masso".into()
}

/// Where a setup's work zero is.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Origin {
    /// The design's own zero, turned with the part, moved by `offset` along the
    /// setup's axes.
    ModelOrigin {
        /// Millimetres along the setup's X, Y and Z.
        #[serde(default, skip_serializing_if = "is_default")]
        offset: [f64; 3],
    },
    /// A point on the box around the setup's stock or models, as the part sits on
    /// the machine, moved by `offset`.
    BoxPoint {
        /// Which box.
        of: BoxOf,
        /// Where along X.
        x: Along,
        /// Where along Y.
        y: Along,
        /// Where along Z.
        z: Along,
        /// Millimetres along the setup's X, Y and Z.
        #[serde(default, skip_serializing_if = "is_default")]
        offset: [f64; 3],
    },
    /// A point picked on a model: a corner, the centre of a round edge, or the
    /// middle of an edge, moved by `offset`.
    Point {
        /// The point.
        point: PointRef,
        /// Millimetres along the setup's X, Y and Z.
        #[serde(default, skip_serializing_if = "is_default")]
        offset: [f64; 3],
    },
}

impl Origin {
    /// Millimetres the work zero is moved by, along the setup's axes.
    pub fn offset(&self) -> [f64; 3] {
        match self {
            Origin::ModelOrigin { offset }
            | Origin::BoxPoint { offset, .. }
            | Origin::Point { offset, .. } => *offset,
        }
    }

    /// The same work zero, moved by `to` instead.
    pub fn with_offset(mut self, to: [f64; 3]) -> Self {
        match &mut self {
            Origin::ModelOrigin { offset }
            | Origin::BoxPoint { offset, .. }
            | Origin::Point { offset, .. } => *offset = to,
        }
        self
    }

    /// The stock's front-left corner at its bottom, so X and Y run into the part
    /// and Z up through it.
    pub fn stock_corner() -> Self {
        Origin::BoxPoint {
            of: BoxOf::Stock,
            x: Along::Min,
            y: Along::Min,
            z: Along::Min,
            offset: [0.0; 3],
        }
    }
}

impl Default for Origin {
    /// The design's own zero, as models are drawn.
    fn default() -> Self {
        Origin::ModelOrigin { offset: [0.0; 3] }
    }
}

/// Which box an origin is placed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BoxOf {
    /// The setup's stock.
    Stock,
    /// The setup's models.
    Model,
}

/// Where along one side of a box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Along {
    /// The low end: left, front or bottom.
    Min,
    /// The middle.
    Center,
    /// The high end: right, back or top.
    Max,
}

/// A face or an edge picked on a model.
///
/// `at` is what the pick is: a point on the feature, in the model's own
/// coordinates, where the person clicked. `index` is the feature's number when it
/// was picked, which finds it at once while the model is unchanged.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct FeatureRef {
    /// The model, by name.
    pub model: String,
    /// The face's or edge's number in the model's topology.
    pub index: u32,
    /// A point on it, in model coordinates, millimetres.
    pub at: [f64; 3],
}

/// Which point of a feature a picked point is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Snap {
    /// A corner, where edges end.
    Vertex,
    /// The centre of a round edge: a hole's, say.
    CircleCenter,
    /// Halfway along an edge.
    EdgeMiddle,
}

/// A point picked on a model.
///
/// As with [`FeatureRef`], `at` is what the pick is: the point itself, in model
/// coordinates. `index` is the vertex's number for [`Snap::Vertex`], and the edge's
/// otherwise.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct PointRef {
    /// The model, by name.
    pub model: String,
    /// Which point of the feature.
    pub snap: Snap,
    /// The vertex's or edge's number in the model's topology.
    pub index: u32,
    /// The point, in model coordinates, millimetres.
    pub at: [f64; 3],
}

/// Where the tool may go, against a boundary made from picked faces or edges.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Containment {
    /// The tool's centre stays inside the boundary, so its side reaches past it by
    /// its radius. Fusion's default.
    #[default]
    Center,
    /// The whole tool stays inside.
    Inside,
    /// The tool may go its whole width outside, so its side still reaches the
    /// boundary from beyond it.
    Outside,
}

impl Along {
    /// The point along `lo..hi`.
    pub fn pick(self, lo: f64, hi: f64) -> f64 {
        match self {
            Along::Min => lo,
            Along::Center => (lo + hi) * 0.5,
            Along::Max => hi,
        }
    }
}

/// A cutter.
///
/// The engine cuts with `kind`, `diameter` and `corner`. Everything after them
/// describes the physical tool the way Fusion's tool library does, for the person
/// setting it up and, later, for feeds and speeds, holder collision in playback,
/// and the machine's tool table. All of it is optional, so a job that names only
/// the cutting geometry is complete.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ToolSpec {
    /// Shape.
    pub kind: ToolKind,
    /// Diameter in millimetres.
    pub diameter: f64,
    /// Corner radius, for a bullnose.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub corner: f64,
    /// Tool number in the program.
    #[serde(default = "one_u32")]
    pub number: u32,
    /// Spindle speed.
    #[serde(default = "default_rpm")]
    pub rpm: u32,
    /// Description for the operator.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
    /// Cutting edges.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flutes: Option<u32>,
    /// Length of the cutting edges, in millimetres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flute_length: Option<f64>,
    /// Length from the tip to where the shank reaches full diameter, in millimetres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shoulder_length: Option<f64>,
    /// Shank diameter, in millimetres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shank_diameter: Option<f64>,
    /// Overall length, in millimetres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overall_length: Option<f64>,
    /// Length out of the holder, tip to holder face, in millimetres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stickout: Option<f64>,
    /// What the cutter is made of.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<ToolMaterial>,
    /// Maker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor: Option<String>,
    /// Maker's product code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product: Option<String>,
    /// What holds it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holder: Option<Holder>,
    /// Suggested feeds for this tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cutting: Option<CuttingData>,
}

impl Default for ToolSpec {
    /// A 6 mm flat end mill: the commonest router cutter, and a sensible start.
    fn default() -> Self {
        Self {
            kind: ToolKind::Flat,
            diameter: 6.0,
            corner: 0.0,
            number: one_u32(),
            rpm: default_rpm(),
            comment: String::new(),
            flutes: None,
            flute_length: None,
            shoulder_length: None,
            shank_diameter: None,
            overall_length: None,
            stickout: None,
            material: None,
            vendor: None,
            product: None,
            holder: None,
            cutting: None,
        }
    }
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// What a cutter is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToolMaterial {
    /// Solid carbide.
    Carbide,
    /// High-speed steel.
    Hss,
}

/// A tool holder, as a stack of cylinders from the holder face upward.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Holder {
    /// Its name, e.g. "ER20 collet chuck".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Cylinders, from the face that meets the tool upward.
    #[serde(default)]
    pub segments: Vec<HolderSegment>,
}

/// One cylinder of a holder.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
pub struct HolderSegment {
    /// Diameter, in millimetres.
    pub diameter: f64,
    /// Length, in millimetres.
    pub length: f64,
}

/// Suggested feeds for a tool. Spindle speed is the tool's own `rpm`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize, Serialize)]
pub struct CuttingData {
    /// Cutting feed, in mm/min.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feed: Option<f64>,
    /// Plunge feed, in mm/min.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plunge_feed: Option<f64>,
    /// Ramp feed, in mm/min.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ramp_feed: Option<f64>,
}

/// A tool field that is not physically meaningful, and why.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{field}: {why}")]
pub struct ToolFieldError {
    /// Which field, as it is named in the file.
    pub field: &'static str,
    /// What is wrong with it.
    pub why: String,
}

fn one_u32() -> u32 {
    1
}

fn default_rpm() -> u32 {
    18000
}

/// Cutter shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToolKind {
    /// Square end.
    Flat,
    /// Ball nose.
    Ball,
    /// Corner radius.
    Bullnose,
}

impl ToolSpec {
    /// Every problem with the tool's fields, each against the field it concerns,
    /// so an editor can show them where they are.
    pub fn check(&self) -> Vec<ToolFieldError> {
        let mut out = Vec::new();
        let mut bad = |field, why: String| out.push(ToolFieldError { field, why });
        if let Err(e) = self.build() {
            let field = if self.diameter.is_finite() && self.diameter > 0.0 {
                "corner"
            } else {
                "diameter"
            };
            bad(field, e.to_string());
        }
        if self.rpm == 0 {
            bad("rpm", "spindle speed must be greater than zero".into());
        }
        if self.flutes == Some(0) {
            bad("flutes", "a cutter has at least one flute".into());
        }
        let lengths = [
            ("flute_length", self.flute_length),
            ("shoulder_length", self.shoulder_length),
            ("shank_diameter", self.shank_diameter),
            ("overall_length", self.overall_length),
            ("stickout", self.stickout),
        ];
        for (field, value) in lengths {
            if let Some(v) = value
                && !(v.is_finite() && v > 0.0)
            {
                bad(field, format!("must be greater than zero, got {v}"));
            }
        }
        // Tip upward: the flutes end before the shoulder, the shoulder before the
        // holder, and what sticks out of the holder is part of the whole tool.
        let order = [
            ("flute_length", self.flute_length),
            ("shoulder_length", self.shoulder_length),
            ("stickout", self.stickout),
            ("overall_length", self.overall_length),
        ];
        let known: Vec<(&'static str, f64)> = order
            .iter()
            .filter_map(|(f, v)| v.map(|v| (*f, v)))
            .collect();
        let words = |field: &str| match field {
            "flute_length" => "the flute length",
            "shoulder_length" => "the shoulder length",
            "stickout" => "the stickout",
            _ => "the overall length",
        };
        for pair in known.windows(2) {
            if let [(shorter, a), (longer, b)] = pair
                && a > b
            {
                bad(
                    shorter,
                    format!("{a} mm is longer than {} ({b} mm)", words(longer)),
                );
            }
        }
        if let Some(h) = &self.holder {
            for (i, seg) in h.segments.iter().enumerate() {
                let ok = |v: f64| v.is_finite() && v > 0.0;
                if !ok(seg.diameter) || !ok(seg.length) {
                    bad(
                        "holder",
                        format!("segment {} needs a positive diameter and length", i + 1),
                    );
                }
            }
        }
        if let Some(c) = &self.cutting {
            for (field, v) in [
                ("cutting.feed", c.feed),
                ("cutting.plunge_feed", c.plunge_feed),
                ("cutting.ramp_feed", c.ramp_feed),
            ] {
                if let Some(v) = v
                    && !(v.is_finite() && v > 0.0)
                {
                    bad(field, format!("must be greater than zero, got {v}"));
                }
            }
        }
        out
    }

    /// The cutter this describes.
    pub fn build(&self) -> Result<Bullnose, ToolError> {
        let r = self.diameter / 2.0;
        match self.kind {
            ToolKind::Flat => Flat::new(r).map(|t| t.as_bullnose()),
            ToolKind::Ball => Ball::new(r).map(|t| t.as_bullnose()),
            ToolKind::Bullnose => Bullnose::new(r, self.corner),
        }
    }
}

/// One operation.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Operation {
    /// Name, for comments and diagnostics.
    pub name: String,
    /// Which strategy, and its own parameters.
    #[serde(flatten)]
    pub strategy: StrategySpec,
    /// Which cutter, by name from the job's tools. A new operation has none yet,
    /// and cannot run until it has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// What to machine.
    #[serde(default, skip_serializing_if = "is_default")]
    pub geometry: Geometry,
    /// Where the tool travels.
    #[serde(default, skip_serializing_if = "is_default")]
    pub heights: Heights,
    /// How the passes are laid out.
    #[serde(default, skip_serializing_if = "is_default")]
    pub passes: Passes,
    /// How the tool moves between cuts.
    #[serde(default, skip_serializing_if = "is_default")]
    pub linking: Linking,
}

/// Strategy and the parameters only it has.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(tag = "strategy", rename_all = "kebab-case")]
pub enum StrategySpec {
    /// Raster finishing.
    Parallel {
        /// Pass direction in degrees; zero runs along +X.
        #[serde(default)]
        angle: f64,
    },
    /// Z-level clearing.
    ZlevelRough {
        /// Depth of each slice, in millimetres.
        stepdown: f64,
        /// Rings shorter than this are not worth the entry.
        #[serde(default = "default_min_ring")]
        min_ring_length: f64,
    },
}

fn default_min_ring() -> f64 {
    3.0
}

/// What to machine.
///
/// With nothing picked, an operation machines the whole part within the stock's
/// footprint. Picked faces and edges keep it to a boundary instead: the outline of
/// the faces seen from above, and the loops the edges close. What else they mean is
/// the strategy's to say, which is why they live here and not with it: a pocket
/// will clear a picked floor, a trace will follow picked edges.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Geometry {
    /// How far the tool may go past what is machined, in millimetres: past the
    /// stock's footprint, or past the picked boundary.
    #[serde(default)]
    pub margin: f64,
    /// Highest level to cut at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<Height>,
    /// Lowest level to cut at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom: Option<Height>,
    /// Faces picked on the models.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faces: Vec<FeatureRef>,
    /// Edges picked on the models.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<FeatureRef>,
    /// Where the tool may go against the picked boundary.
    #[serde(default, skip_serializing_if = "is_default")]
    pub containment: Containment,
}

impl Default for Geometry {
    fn default() -> Self {
        Self {
            margin: 0.0,
            top: None,
            bottom: None,
            faces: Vec::new(),
            edges: Vec::new(),
            containment: Containment::Center,
        }
    }
}

impl Geometry {
    /// Whether any face or edge is picked.
    pub fn picked(&self) -> bool {
        !self.faces.is_empty() || !self.edges.is_empty()
    }
}

/// Where the tool travels.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Heights {
    /// Traverse height.
    #[serde(default = "default_clearance")]
    pub clearance: Height,
}

impl Default for Heights {
    fn default() -> Self {
        Self {
            clearance: default_clearance(),
        }
    }
}

fn default_clearance() -> Height {
    Height {
        reference: HeightRef::StockTop,
        offset: 10.0,
    }
}

/// A height, stated relative to something that can be measured.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
pub struct Height {
    /// What the offset is measured from.
    #[serde(rename = "ref", default = "default_ref")]
    pub reference: HeightRef,
    /// Millimetres above the reference.
    #[serde(default)]
    pub offset: f64,
}

fn default_ref() -> HeightRef {
    HeightRef::StockTop
}

/// What a height is measured from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HeightRef {
    /// Top of the block.
    StockTop,
    /// Bottom of the block.
    StockBottom,
    /// Highest point of the model.
    ModelTop,
    /// Lowest point of the model.
    ModelBottom,
    /// The offset is the height, in setup coordinates.
    Absolute,
}

/// What a height resolves against.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeightContext {
    /// Top of the block.
    pub stock_top: f64,
    /// Bottom of the block.
    pub stock_bottom: f64,
    /// Highest point of the model.
    pub model_top: f64,
    /// Lowest point of the model.
    pub model_bottom: f64,
}

impl Height {
    /// Resolve to a height in setup coordinates.
    pub fn resolve(&self, ctx: &HeightContext) -> f64 {
        let base = match self.reference {
            HeightRef::StockTop => ctx.stock_top,
            HeightRef::StockBottom => ctx.stock_bottom,
            HeightRef::ModelTop => ctx.model_top,
            HeightRef::ModelBottom => ctx.model_bottom,
            HeightRef::Absolute => 0.0,
        };
        base + self.offset
    }
}

/// How the passes are laid out.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Passes {
    /// Distance between neighbouring passes, in millimetres.
    #[serde(default = "default_stepover")]
    pub stepover: f64,
    /// Cutting feed, in mm/min.
    #[serde(default = "default_feed")]
    pub feed: f64,
    /// Material to leave behind.
    #[serde(default)]
    pub stock_to_leave: StockToLeave,
    /// Largest Z drop that may be traversed as a cut.
    #[serde(default = "default_max_cut_step")]
    pub max_cut_step: f64,
}

impl Default for Passes {
    fn default() -> Self {
        Self {
            stepover: default_stepover(),
            feed: default_feed(),
            stock_to_leave: StockToLeave::default(),
            max_cut_step: default_max_cut_step(),
        }
    }
}

fn default_stepover() -> f64 {
    1.0
}

fn default_feed() -> f64 {
    1200.0
}

fn default_max_cut_step() -> f64 {
    0.5
}

/// Material to leave, radial and axial.
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize, Serialize)]
pub struct StockToLeave {
    /// Perpendicular to the tool axis.
    #[serde(default)]
    pub radial: f64,
    /// Along the tool axis.
    #[serde(default)]
    pub axial: f64,
}

/// How the tool moves between cuts.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Linking {
    /// Feed for coming up out of a cut.
    #[serde(default = "default_retract_feed")]
    pub retract_feed: f64,
    /// Feed for descending into material.
    #[serde(default = "default_plunge_feed")]
    pub plunge_feed: f64,
    /// Whether passes may be reordered to shorten travel.
    #[serde(default = "yes")]
    pub reorder: bool,
    /// Whether an open pass may be reversed while reordering.
    ///
    /// Off by default: reversing flips climb for conventional, and that is a cutting
    /// decision rather than a travel one.
    #[serde(default)]
    pub allow_reverse: bool,
}

impl Default for Linking {
    fn default() -> Self {
        Self {
            retract_feed: default_retract_feed(),
            plunge_feed: default_plunge_feed(),
            reorder: true,
            allow_reverse: false,
        }
    }
}

fn default_retract_feed() -> f64 {
    1500.0
}

fn default_plunge_feed() -> f64 {
    300.0
}

fn yes() -> bool {
    true
}

/// A job that cannot be run as written.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum JobError {
    /// The file is a format this build does not read.
    #[error("job format {0} is not one this version reads (it reads 1 to {JOB_VERSION})")]
    UnsupportedVersion(u32),
    /// Something was left without a name.
    #[error("a {what} has no name")]
    NoName {
        /// What kind of thing.
        what: &'static str,
    },
    /// Two things of one kind share a name, so a reference to it is ambiguous.
    #[error("there are two {what}s called '{name}'")]
    DuplicateName {
        /// What kind of thing.
        what: &'static str,
        /// The name.
        name: String,
    },
    /// A setup or a stock names a model the job does not have.
    #[error("{owner} uses model '{model}', which is not in the job")]
    UnknownModel {
        /// Who asked for it.
        owner: String,
        /// Which model.
        model: String,
    },
    /// A setup names a stock the job does not have.
    #[error("setup '{setup}' uses stock '{stock}', which is not in the job")]
    UnknownStock {
        /// Which setup.
        setup: String,
        /// Which stock.
        stock: String,
    },
    /// A setup machines no model.
    #[error("setup '{0}' has no model")]
    NoModels(String),
    /// A setup has no stock.
    #[error("setup '{0}' has no stock")]
    NoStock(String),
    /// The job has no setups.
    #[error("job has no setups")]
    NoSetups,
    /// A setup would do nothing.
    #[error("setup '{0}' has no operations")]
    NoOperations(String),
    /// A setup's axes are not square to each other.
    #[error("setup '{setup}': {source}")]
    BadOrientation {
        /// Which setup.
        setup: String,
        /// Why.
        source: BadOrientation,
    },
    /// An operation has no tool yet.
    #[error("operation '{op}' has no tool")]
    NoTool {
        /// Which operation, as `setup/operation`.
        op: String,
    },
    /// An operation named a tool the job does not have.
    #[error("operation '{op}' uses tool '{tool}', which is not in the job's tools")]
    UnknownTool {
        /// Which operation, as `setup/operation`.
        op: String,
        /// Which tool it asked for.
        tool: String,
    },
    /// A tool dimension was not physically meaningful.
    #[error("tool '{name}': {source}")]
    BadTool {
        /// Which tool.
        name: String,
        /// Why.
        source: ToolError,
    },
    /// A value was outside its usable range.
    #[error("{field} must be finite and greater than zero, got {value}")]
    BadValue {
        /// Which field.
        field: String,
        /// What it was.
        value: f64,
    },
    /// A work offset outside the usual range.
    #[error("setup '{setup}': work offset {wcs} is not in the range 54..=59")]
    BadWorkOffset {
        /// Which setup.
        setup: String,
        /// The offset.
        wcs: u8,
    },
    /// A tool field that is not physically meaningful.
    #[error("tool '{name}', {source}")]
    BadToolField {
        /// Which tool.
        name: String,
        /// What is wrong.
        source: ToolFieldError,
    },
    /// A pick names a model the setup does not machine.
    #[error("{owner} was picked on model '{model}', which setup '{setup}' does not machine")]
    PickedFromOtherModel {
        /// Which setup.
        setup: String,
        /// What it was picked for.
        owner: String,
        /// The model it names.
        model: String,
    },
    /// A pick's point is not a point.
    #[error("{owner}: a picked point is not finite: {at:?}")]
    BadPick {
        /// What it was picked for.
        owner: String,
        /// The point.
        at: [f64; 3],
    },
    /// Two different tools the job uses share one tool number. The machine would
    /// load one cutter and run the other's program.
    #[error(
        "tools '{first}' and '{second}' are both T{number}; the machine would load one \
         and cut with the other's program"
    )]
    DuplicateToolNumber {
        /// The number.
        number: u32,
        /// One tool.
        first: String,
        /// The other.
        second: String,
    },
}

impl Job {
    /// Check the whole job is runnable before anything is computed: everything
    /// [`check_common`](Self::check_common) checks, and every setup.
    ///
    /// Everything catchable is caught here, so a job fails on its own terms rather
    /// than part-way through an operation with a half-built path in hand.
    pub fn validate(&self) -> Result<(), JobError> {
        self.check_common()?;
        if self.setups.is_empty() {
            return Err(JobError::NoSetups);
        }
        (0..self.setups.len()).try_for_each(|i| self.check_setup(i))
    }

    /// Check what every setup depends on: the format, the tolerance, the tools,
    /// the names, and the stocks. A setup still being written does not fail this,
    /// so another setup can run.
    pub fn check_common(&self) -> Result<(), JobError> {
        if !(1..=JOB_VERSION).contains(&self.version) {
            return Err(JobError::UnsupportedVersion(self.version));
        }
        positive("tolerance", self.tolerance)?;

        for (name, spec) in &self.tools {
            spec.build().map_err(|source| JobError::BadTool {
                name: name.clone(),
                source,
            })?;
            if spec.rpm == 0 {
                return Err(JobError::BadValue {
                    field: format!("tools.{name}.rpm"),
                    value: 0.0,
                });
            }
            if let Some(source) = spec.check().into_iter().next() {
                return Err(JobError::BadToolField {
                    name: name.clone(),
                    source,
                });
            }
        }

        unique("model", self.models.iter().map(|m| m.name.as_str()))?;
        unique("stock", self.stocks.iter().map(|s| s.name.as_str()))?;
        unique("setup", self.setups.iter().map(|s| s.name.as_str()))?;

        for stock in &self.stocks {
            let field = |f: &str| format!("stock '{}'.{f}", stock.name);
            match &stock.spec {
                StockSpec::AroundModels {
                    models,
                    margin,
                    below,
                } => {
                    for m in models {
                        if self.model(m).is_none() {
                            return Err(JobError::UnknownModel {
                                owner: format!("stock '{}'", stock.name),
                                model: m.clone(),
                            });
                        }
                    }
                    for (i, v) in margin.iter().chain(std::iter::once(below)).enumerate() {
                        if !v.is_finite() || *v < 0.0 {
                            return Err(JobError::BadValue {
                                field: field(if i < 3 { "margin" } else { "below" }),
                                value: *v,
                            });
                        }
                    }
                }
                StockSpec::FixedBox { corner, size } => {
                    for v in corner {
                        if !v.is_finite() {
                            return Err(JobError::BadValue {
                                field: field("corner"),
                                value: *v,
                            });
                        }
                    }
                    for v in size {
                        positive(&field("size"), *v)?;
                    }
                }
            }
        }

        // Only tools the operations use reach the machine, so only they must not
        // share a number. A table may keep a spare with any number it likes. The
        // machine's tool table is one, so this is across every setup.
        let mut used: BTreeMap<u32, &str> = BTreeMap::new();
        for op in self.setups.iter().flat_map(|s| &s.operations) {
            let Some((name, spec)) = op.tool.as_deref().and_then(|t| self.tools.get_key_value(t))
            else {
                continue;
            };
            match used.get(&spec.number) {
                Some(first) if *first != name.as_str() => {
                    return Err(JobError::DuplicateToolNumber {
                        number: spec.number,
                        first: (*first).to_string(),
                        second: name.clone(),
                    });
                }
                _ => {
                    used.insert(spec.number, name);
                }
            }
        }
        Ok(())
    }

    /// Check setup `index` can run, given [`check_common`](Self::check_common)
    /// passes. An index past the end is [`JobError::NoSetups`].
    pub fn check_setup(&self, index: usize) -> Result<(), JobError> {
        let setup = self.setups.get(index).ok_or(JobError::NoSetups)?;
        let name = &setup.name;
        setup
            .orientation
            .check()
            .map_err(|source| JobError::BadOrientation {
                setup: name.clone(),
                source,
            })?;
        if !(54..=59).contains(&setup.wcs) {
            return Err(JobError::BadWorkOffset {
                setup: name.clone(),
                wcs: setup.wcs,
            });
        }
        if setup.models.is_empty() {
            return Err(JobError::NoModels(name.clone()));
        }
        for m in &setup.models {
            if self.model(m).is_none() {
                return Err(JobError::UnknownModel {
                    owner: format!("setup '{name}'"),
                    model: m.clone(),
                });
            }
        }
        if setup.stocks.is_empty() {
            return Err(JobError::NoStock(name.clone()));
        }
        for s in &setup.stocks {
            if self.stock(s).is_none() {
                return Err(JobError::UnknownStock {
                    setup: name.clone(),
                    stock: s.clone(),
                });
            }
        }
        if setup.operations.is_empty() {
            return Err(JobError::NoOperations(name.clone()));
        }
        let pick = |owner: String, model: &str, at: &[f64; 3]| {
            if !setup.models.iter().any(|m| m == model) {
                return Err(JobError::PickedFromOtherModel {
                    setup: name.clone(),
                    owner,
                    model: model.to_string(),
                });
            }
            if !at.iter().all(|v| v.is_finite()) {
                return Err(JobError::BadPick { owner, at: *at });
            }
            Ok(())
        };
        if let Origin::Point { point, .. } = &setup.origin {
            pick(format!("setup '{name}' work zero"), &point.model, &point.at)?;
        }
        for op in &setup.operations {
            for r in op.geometry.faces.iter().chain(&op.geometry.edges) {
                pick(format!("operation '{name}/{}'", op.name), &r.model, &r.at)?;
            }
        }

        for op in &setup.operations {
            let at = format!("{name}/{}", op.name);
            let Some(tool) = &op.tool else {
                return Err(JobError::NoTool { op: at });
            };
            if !self.tools.contains_key(tool) {
                return Err(JobError::UnknownTool {
                    op: at,
                    tool: tool.clone(),
                });
            }
            positive(&format!("{at}.passes.stepover"), op.passes.stepover)?;
            positive(&format!("{at}.passes.feed"), op.passes.feed)?;
            positive(&format!("{at}.passes.max_cut_step"), op.passes.max_cut_step)?;
            positive(
                &format!("{at}.linking.retract_feed"),
                op.linking.retract_feed,
            )?;
            positive(&format!("{at}.linking.plunge_feed"), op.linking.plunge_feed)?;
            if op.passes.stock_to_leave.radial < 0.0 || op.passes.stock_to_leave.axial < 0.0 {
                return Err(JobError::BadValue {
                    field: format!("{at}.passes.stock_to_leave"),
                    value: op
                        .passes
                        .stock_to_leave
                        .radial
                        .min(op.passes.stock_to_leave.axial),
                });
            }
            if let StrategySpec::ZlevelRough { stepdown, .. } = op.strategy {
                positive(&format!("{at}.stepdown"), stepdown)?;
            }
        }
        Ok(())
    }

    /// The oldest file format that holds everything this job says: 2 if it uses
    /// picked features, which a version 1 reader would silently skip, and 1
    /// otherwise.
    pub fn required_version(&self) -> u32 {
        let picks = self.setups.iter().any(|s| {
            matches!(s.origin, Origin::Point { .. })
                || s.operations.iter().any(|op| op.geometry.picked())
        });
        if picks { 2 } else { 1 }
    }

    /// Every pick on model `from` names model `to` instead, for when a model is
    /// renamed.
    pub fn rename_model_refs(&mut self, from: &str, to: &str) {
        for setup in &mut self.setups {
            if let Origin::Point { point, .. } = &mut setup.origin
                && point.model == from
            {
                point.model = to.to_string();
            }
            for op in &mut setup.operations {
                let g = &mut op.geometry;
                for r in g.faces.iter_mut().chain(&mut g.edges) {
                    if r.model == from {
                        r.model = to.to_string();
                    }
                }
            }
        }
    }

    /// The tool an operation names, if it names one the job has.
    pub fn tool_for(&self, op: &Operation) -> Option<&ToolSpec> {
        op.tool.as_deref().and_then(|t| self.tools.get(t))
    }

    /// The model called `name`.
    pub fn model(&self, name: &str) -> Option<&ModelDef> {
        self.models.iter().find(|m| m.name == name)
    }

    /// The stock called `name`.
    pub fn stock(&self, name: &str) -> Option<&StockDef> {
        self.stocks.iter().find(|s| s.name == name)
    }

    /// Every model setup `index` needs loaded: its own, and those its stocks are
    /// built around. Each once, in the job's order.
    pub fn models_for(&self, index: usize) -> Vec<&ModelDef> {
        let Some(setup) = self.setups.get(index) else {
            return Vec::new();
        };
        let mut want: BTreeSet<&str> = setup.models.iter().map(String::as_str).collect();
        for s in setup.stocks.iter().filter_map(|s| self.stock(s)) {
            if let StockSpec::AroundModels { models, .. } = &s.spec {
                want.extend(models.iter().map(String::as_str));
            }
        }
        self.models
            .iter()
            .filter(|m| want.contains(m.name.as_str()))
            .collect()
    }
}

fn unique<'a>(what: &'static str, names: impl Iterator<Item = &'a str>) -> Result<(), JobError> {
    let mut seen = BTreeSet::new();
    for n in names {
        if n.trim().is_empty() {
            return Err(JobError::NoName { what });
        }
        if !seen.insert(n) {
            return Err(JobError::DuplicateName {
                what,
                name: n.to_string(),
            });
        }
    }
    Ok(())
}

fn positive(field: &str, value: f64) -> Result<(), JobError> {
    (value.is_finite() && value > 0.0)
        .then_some(())
        .ok_or_else(|| JobError::BadValue {
            field: field.to_string(),
            value,
        })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use crate::frame::Axis;
    use crate::tool::ToolProfile;

    const SAMPLE: &str = r#"
version = 1
tolerance = 0.02

[tools.rough-6-flat]
kind = "flat"
diameter = 6.0
number = 1
rpm = 16000
comment = "6mm flat, carbide"

[tools.finish-4-ball]
kind = "ball"
diameter = 4.0
number = 2

[[model]]
name = "part"
path = "part.step"

[[stock]]
name = "block"
kind = "around-models"
margin = [2.0, 2.0, 1.0]

[[setup]]
name = "top"
models = ["part"]
stocks = ["block"]
wcs = 54

[[setup.operation]]
name = "rough"
strategy = "zlevel-rough"
tool = "rough-6-flat"
stepdown = 2.0
geometry = { margin = 1.0, bottom = { ref = "model-bottom" } }
heights = { clearance = { ref = "stock-top", offset = 8.0 } }
passes = { stepover = 3.0, feed = 1800.0, stock_to_leave = { radial = 0.5, axial = 0.2 } }
linking = { retract_feed = 1800.0, plunge_feed = 400.0 }

[[setup.operation]]
name = "finish"
strategy = "parallel"
tool = "finish-4-ball"
angle = 90.0
passes = { stepover = 0.4, feed = 2400.0 }
"#;

    fn sample() -> Job {
        toml::from_str(SAMPLE).unwrap()
    }

    #[test]
    fn parses_a_complete_job() {
        let j = sample();
        assert_eq!(j.tolerance, 0.02);
        assert_eq!(j.tools.len(), 2);
        assert_eq!(j.setups.len(), 1);
        assert_eq!(j.setups[0].operations.len(), 2);
        j.validate().unwrap();
    }

    #[test]
    fn defaults_fill_in_what_is_omitted() {
        let minimal = r#"
version = 1
[tools.t]
kind = "ball"
diameter = 6.0
[[model]]
name = "m"
path = "m.step"
[[stock]]
name = "s"
kind = "around-models"
[[setup]]
name = "top"
models = ["m"]
stocks = ["s"]
[[setup.operation]]
name = "finish"
strategy = "parallel"
tool = "t"
"#;
        let j: Job = toml::from_str(minimal).unwrap();
        j.validate().unwrap();
        assert_eq!(j.tolerance, 0.01);
        let setup = &j.setups[0];
        assert_eq!(setup.wcs, 54);
        assert_eq!(setup.post, "masso");
        assert_eq!(setup.orientation, Orientation::default());
        assert_eq!(setup.origin, Origin::default());
        let op = setup.operations.first().unwrap();
        assert_eq!(op.passes.stepover, 1.0);
        assert!(op.linking.reorder);
        assert!(
            !op.linking.allow_reverse,
            "reversing flips climb for conventional and must be asked for"
        );
    }

    /// Everything a job can say survives being written and read back, and the
    /// written file leaves out what is only default.
    #[test]
    fn a_job_round_trips() {
        let mut j = sample();
        j.stocks.push(StockDef {
            name: "plate".into(),
            spec: StockSpec::FixedBox {
                corner: [-5.0, -5.0, -12.5],
                size: [110.0, 60.0, 25.0],
            },
        });
        j.models.push(ModelDef {
            name: "stl".into(),
            path: "../meshes/other.stl".into(),
            units: Some(LengthUnit::Inch),
        });
        let mut bottom = Setup::new("bottom");
        bottom.models = vec!["part".into(), "stl".into()];
        bottom.stocks = vec!["block".into(), "plate".into()];
        bottom.wcs = 55;
        bottom.post = "profiles/mine.toml".into();
        bottom.orientation = Orientation {
            z: Axis::NegZ,
            x: Axis::PosY,
        };
        bottom.origin = Origin::BoxPoint {
            of: BoxOf::Stock,
            x: Along::Min,
            y: Along::Center,
            z: Along::Max,
            offset: [0.0, 0.0, -1.5],
        };
        let mut unfinished = j.setups[0].operations[1].clone();
        unfinished.tool = None;
        bottom.operations = vec![unfinished];
        j.setups.push(bottom);
        j.setups[0].origin = Origin::ModelOrigin {
            offset: [1.0, 2.0, 3.0],
        };

        let text = toml::to_string(&j).unwrap();
        let again: Job = toml::from_str(&text).unwrap();
        assert_eq!(again, j, "{text}");
        // Defaults are not written out.
        let plain = toml::to_string(&sample()).unwrap();
        assert!(!plain.contains("orientation"), "{plain}");
        assert!(!plain.contains("below"), "{plain}");
    }

    /// A file without a version is refused by name, not with a parse error about
    /// some field.
    #[test]
    fn another_format_is_refused() {
        let mut j = sample();
        j.version = 7;
        assert_eq!(j.validate(), Err(JobError::UnsupportedVersion(7)));
    }

    /// The reason heights are symbolic: the same job survives a different block.
    #[test]
    fn heights_resolve_against_the_setup() {
        let ctx = HeightContext {
            stock_top: 20.0,
            stock_bottom: -5.0,
            model_top: 18.0,
            model_bottom: 0.0,
        };
        let cases = [
            (HeightRef::StockTop, 8.0, 28.0),
            (HeightRef::StockBottom, 0.0, -5.0),
            (HeightRef::ModelTop, 2.0, 20.0),
            (HeightRef::ModelBottom, -1.0, -1.0),
            (HeightRef::Absolute, 12.5, 12.5),
        ];
        for (reference, offset, want) in cases {
            let h = Height { reference, offset };
            assert_eq!(h.resolve(&ctx), want, "{reference:?} + {offset}");
        }
    }

    #[test]
    fn a_taller_block_moves_the_clearance_with_it() {
        let h = Height {
            reference: HeightRef::StockTop,
            offset: 10.0,
        };
        let short = HeightContext {
            stock_top: 10.0,
            stock_bottom: 0.0,
            model_top: 8.0,
            model_bottom: 0.0,
        };
        let tall = HeightContext {
            stock_top: 50.0,
            ..short
        };
        assert_eq!(h.resolve(&short), 20.0);
        assert_eq!(h.resolve(&tall), 60.0);
    }

    #[test]
    fn tools_build_into_the_bullnose_family() {
        let j = sample();
        let flat = j.tools.get("rough-6-flat").unwrap().build().unwrap();
        assert_eq!(flat.max_radius(), 3.0);
        assert_eq!(flat.corner(), 0.0, "a flat mill has no corner");

        let ball = j.tools.get("finish-4-ball").unwrap().build().unwrap();
        assert_eq!(ball.corner(), 2.0, "a ball mill's corner fills its radius");
    }

    #[test]
    fn an_unknown_tool_is_caught_before_anything_runs() {
        let bad = SAMPLE.replace(r#"tool = "finish-4-ball""#, r#"tool = "nonesuch""#);
        let j: Job = toml::from_str(&bad).unwrap();
        assert_eq!(
            j.validate(),
            Err(JobError::UnknownTool {
                op: "top/finish".into(),
                tool: "nonesuch".into()
            })
        );
    }

    /// A new operation has no tool; it saves, but it does not run.
    #[test]
    fn an_operation_without_a_tool_does_not_run() {
        let mut j = sample();
        j.setups[0].operations[1].tool = None;
        assert!(j.check_common().is_ok());
        assert_eq!(
            j.validate(),
            Err(JobError::NoTool {
                op: "top/finish".into()
            })
        );
    }

    /// Catchable mistakes are caught up front, not part-way through an operation
    /// with a half-built path in hand.
    #[test]
    fn unusable_values_are_rejected_up_front() {
        for (from, to) in [
            ("stepover = 0.4", "stepover = 0.0"),
            ("feed = 2400.0", "feed = -1.0"),
            ("tolerance = 0.02", "tolerance = 0.0"),
            ("wcs = 54", "wcs = 99"),
            ("stepdown = 2.0", "stepdown = 0.0"),
            ("margin = [2.0, 2.0, 1.0]", "margin = [2.0, -2.0, 1.0]"),
        ] {
            let j: Job = toml::from_str(&SAMPLE.replace(from, to)).unwrap();
            assert!(j.validate().is_err(), "{to} should be rejected");
        }
    }

    /// Setups and stocks name what they use, and a name that is not there, or is
    /// there twice, stops the job.
    #[test]
    fn references_must_resolve() {
        let cases = [
            (r#"models = ["part"]"#, r#"models = ["nope"]"#),
            (r#"stocks = ["block"]"#, r#"stocks = ["nope"]"#),
            (r#"models = ["part"]"#, "models = []"),
            (r#"stocks = ["block"]"#, "stocks = []"),
        ];
        for (from, to) in cases {
            let j: Job = toml::from_str(&SAMPLE.replace(from, to)).unwrap();
            assert!(j.validate().is_err(), "{to} should be rejected");
        }
        let mut j = sample();
        j.stocks[0].spec = StockSpec::AroundModels {
            models: vec!["ghost".into()],
            margin: [0.0; 3],
            below: 0.0,
        };
        assert!(matches!(
            j.check_common(),
            Err(JobError::UnknownModel { .. })
        ));
        let mut j = sample();
        j.setups.push(j.setups[0].clone());
        assert!(matches!(
            j.check_common(),
            Err(JobError::DuplicateName { what: "setup", .. })
        ));
    }

    /// A setup still being written does not stop another from running.
    #[test]
    fn one_setup_is_checked_on_its_own() {
        let mut j = sample();
        j.setups.push(Setup::new("bottom"));
        j.check_common().unwrap();
        j.check_setup(0).unwrap();
        assert_eq!(j.check_setup(1), Err(JobError::NoModels("bottom".into())));
        assert!(j.validate().is_err());
    }

    #[test]
    fn a_setup_with_no_operations_is_refused() {
        let mut j = sample();
        j.setups[0].operations.clear();
        assert_eq!(j.validate(), Err(JobError::NoOperations("top".into())));
        j.setups.clear();
        assert_eq!(j.validate(), Err(JobError::NoSetups));
    }

    #[test]
    fn a_bad_orientation_is_refused() {
        let mut j = sample();
        j.setups[0].orientation = Orientation {
            z: Axis::PosX,
            x: Axis::NegX,
        };
        assert!(matches!(j.validate(), Err(JobError::BadOrientation { .. })));
    }

    /// A setup needs its own models and those its stocks are around.
    #[test]
    fn a_setup_needs_the_models_its_stock_is_around() {
        let mut j = sample();
        j.models.push(ModelDef {
            name: "fixture".into(),
            path: "fixture.step".into(),
            units: None,
        });
        j.stocks[0].spec = StockSpec::AroundModels {
            models: vec!["fixture".into()],
            margin: [0.0; 3],
            below: 0.0,
        };
        let names: Vec<&str> = j.models_for(0).iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["part", "fixture"]);
    }

    #[test]
    fn strategy_parameters_belong_to_their_strategy() {
        let j = sample();
        let ops = &j.setups[0].operations;
        match ops[0].strategy {
            StrategySpec::ZlevelRough { stepdown, .. } => assert_eq!(stepdown, 2.0),
            StrategySpec::Parallel { .. } => panic!("first operation should be roughing"),
        }
        match ops[1].strategy {
            StrategySpec::Parallel { angle } => assert_eq!(angle, 90.0),
            StrategySpec::ZlevelRough { .. } => panic!("second operation should be finishing"),
        }
    }

    /// Every shipped example parses and validates.
    #[test]
    fn the_examples_validate() {
        let job: Job = toml::from_str(include_str!("../../../examples/rail-jig.camcraft")).unwrap();
        job.validate().unwrap();
        sample().validate().unwrap();
    }

    const FULL_TOOL: &str = r#"
kind = "bullnose"
diameter = 6.0
corner = 0.5
number = 3
rpm = 16000
comment = "6mm bullnose"
flutes = 2
flute_length = 15.0
shoulder_length = 20.0
shank_diameter = 6.0
overall_length = 50.0
stickout = 25.0
material = "carbide"
vendor = "Amana"
product = "46170-K"
holder = { name = "ER20", segments = [{ diameter = 32.0, length = 20.0 }, { diameter = 40.0, length = 30.0 }] }
cutting = { feed = 2000.0, plunge_feed = 400.0 }
"#;

    /// A tool described the way Fusion describes one reads, checks clean and writes
    /// back unchanged, which is what copying between a library and a job relies on.
    #[test]
    fn a_fully_described_tool_round_trips() {
        let tool: ToolSpec = toml::from_str(FULL_TOOL).unwrap();
        assert!(tool.check().is_empty(), "{:?}", tool.check());
        assert_eq!(tool.material, Some(ToolMaterial::Carbide));
        assert_eq!(tool.holder.as_ref().unwrap().segments.len(), 2);
        let again: ToolSpec = toml::from_str(&toml::to_string(&tool).unwrap()).unwrap();
        assert_eq!(again, tool);
        // Absent stays absent: a minimal tool writes only what it has.
        let minimal = toml::to_string(&ToolSpec::default()).unwrap();
        assert!(
            !minimal.contains("flutes") && !minimal.contains("corner"),
            "{minimal}"
        );
    }

    fn with(edit: impl FnOnce(&mut ToolSpec)) -> Vec<&'static str> {
        let mut tool: ToolSpec = toml::from_str(FULL_TOOL).unwrap();
        edit(&mut tool);
        tool.check().into_iter().map(|e| e.field).collect()
    }

    /// Each rule names the field it is about, so an editor can show it there.
    #[test]
    fn tool_checks_name_the_field_at_fault() {
        assert_eq!(with(|t| t.flutes = Some(0)), ["flutes"]);
        assert_eq!(with(|t| t.flute_length = Some(30.0)), ["flute_length"]);
        assert_eq!(with(|t| t.stickout = Some(60.0)), ["stickout"]);
        assert_eq!(with(|t| t.shank_diameter = Some(-1.0)), ["shank_diameter"]);
        assert_eq!(with(|t| t.corner = 4.0), ["corner"]);
        assert_eq!(with(|t| t.diameter = 0.0), ["diameter"]);
        assert_eq!(with(|t| t.rpm = 0), ["rpm"]);
        assert_eq!(
            with(|t| t.cutting = Some(CuttingData {
                feed: Some(0.0),
                ..Default::default()
            })),
            ["cutting.feed"]
        );
        assert_eq!(
            with(|t| t.holder.as_mut().unwrap().segments[0].length = 0.0),
            ["holder"]
        );
        // Missing lengths are not wrong, only unknown.
        assert!(with(|t| t.shoulder_length = None).is_empty());
    }

    /// A bad field stops the job, as a bad diameter always has.
    #[test]
    fn a_bad_tool_field_stops_the_job() {
        let bad = SAMPLE.replace(
            "[tools.rough-6-flat]\n",
            "[tools.rough-6-flat]\nflutes = 0\n",
        );
        assert_ne!(bad, SAMPLE, "the edit must land");
        let job: Job = toml::from_str(&bad).unwrap();
        assert!(matches!(
            job.validate(),
            Err(JobError::BadToolField { ref source, .. }) if source.field == "flutes"
        ));
    }

    /// Two tools the job uses may not share a number, in any setup; a spare in the
    /// table may, and one tool used by two operations is one tool.
    #[test]
    fn two_used_tools_may_not_share_a_number() {
        let clash = SAMPLE.replace("number = 2", "number = 1");
        assert_ne!(clash, SAMPLE, "the edit must land");
        let job: Job = toml::from_str(&clash).unwrap();
        assert!(matches!(
            job.validate(),
            Err(JobError::DuplicateToolNumber { number: 1, .. })
        ));

        let spare =
            format!("{SAMPLE}\n[tools.spare]\nkind = \"flat\"\ndiameter = 3.0\nnumber = 1\n");
        let job: Job = toml::from_str(&spare).unwrap();
        job.validate().unwrap();

        let mut shared = sample();
        let tool = shared.setups[0].operations[0].tool.clone();
        shared.setups[0].operations[1].tool = tool;
        shared.validate().unwrap();

        // Across setups: the machine has one tool table.
        let mut two = sample();
        let mut other = two.setups[0].clone();
        other.name = "bottom".into();
        two.setups.push(other);
        two.tools.get_mut("finish-4-ball").unwrap().number = 1;
        two.setups[0].operations.truncate(1);
        two.setups[1].operations.remove(0);
        assert!(matches!(
            two.validate(),
            Err(JobError::DuplicateToolNumber { number: 1, .. })
        ));
    }

    fn picked() -> Job {
        let mut j = sample();
        let s = &mut j.setups[0];
        s.origin = Origin::Point {
            point: PointRef {
                model: "part".into(),
                snap: Snap::CircleCenter,
                index: 41,
                at: [12.7, 6.35, 25.4],
            },
            offset: [0.0, 0.0, 1.0],
        };
        let g = &mut s.operations[1].geometry;
        g.faces.push(FeatureRef {
            model: "part".into(),
            index: 12,
            at: [30.2, 14.0, 12.7],
        });
        g.edges.push(FeatureRef {
            model: "part".into(),
            index: 3,
            at: [1.0, 2.0, 3.0],
        });
        g.containment = Containment::Inside;
        j
    }

    /// Picks survive being written and read back, and are written as the plan's
    /// TOML shows them.
    #[test]
    fn picks_round_trip() {
        let j = picked();
        j.validate().unwrap();
        let text = toml::to_string(&j).unwrap();
        let again: Job = toml::from_str(&text).unwrap();
        assert_eq!(again, j, "{text}");
        assert!(text.contains(r#"kind = "point""#), "{text}");
        assert!(text.contains(r#"snap = "circle-center""#), "{text}");
        assert!(text.contains(r#"containment = "inside""#), "{text}");
        assert!(
            text.contains("[[setup.operation.geometry.faces]]"),
            "{text}"
        );
        // Nothing picked, nothing written.
        let plain = toml::to_string(&sample()).unwrap();
        assert!(!plain.contains("faces"), "{plain}");
        assert!(!plain.contains("containment"), "{plain}");
    }

    /// A version 1 reader would skip picks and cut the whole part, so a job with
    /// any is version 2; every other job stays version 1, readable as before.
    #[test]
    fn only_a_job_with_picks_needs_version_2() {
        assert_eq!(sample().required_version(), 1);
        assert_eq!(picked().required_version(), 2);
        let mut origin_only = sample();
        origin_only.setups[0].origin = picked().setups[0].origin.clone();
        assert_eq!(origin_only.required_version(), 2);
        let mut faces_only = picked();
        faces_only.setups[0].origin = Origin::default();
        faces_only.setups[0].operations[1].geometry.edges.clear();
        assert_eq!(faces_only.required_version(), 2);
        // Both versions are read.
        let mut one = sample();
        one.version = 1;
        one.validate().unwrap();
        let mut two = sample();
        two.version = 2;
        two.validate().unwrap();
        let mut three = sample();
        three.version = 3;
        assert_eq!(three.validate(), Err(JobError::UnsupportedVersion(3)));
    }

    #[test]
    fn a_pick_must_be_on_the_setup_and_a_point() {
        let mut j = picked();
        j.setups[0].operations[1].geometry.faces[0].model = "elsewhere".into();
        assert!(matches!(
            j.validate(),
            Err(JobError::PickedFromOtherModel { model, .. }) if model == "elsewhere"
        ));
        let mut j = picked();
        if let Origin::Point { point, .. } = &mut j.setups[0].origin {
            point.at[1] = f64::NAN;
        }
        assert!(matches!(j.validate(), Err(JobError::BadPick { .. })));
    }

    #[test]
    fn renaming_a_model_carries_its_picks() {
        let mut j = picked();
        j.rename_model_refs("part", "bracket");
        let s = &j.setups[0];
        assert!(matches!(&s.origin, Origin::Point { point, .. } if point.model == "bracket"));
        let g = &s.operations[1].geometry;
        assert!(g.faces.iter().chain(&g.edges).all(|r| r.model == "bracket"));
        j.rename_model_refs("nothing", "else");
        assert_eq!(j.setups[0].operations[1].geometry.faces[0].model, "bracket");
    }

    #[test]
    fn an_origin_keeps_its_kind_when_its_offset_changes() {
        let o = picked().setups[0].origin.clone();
        assert_eq!(o.offset(), [0.0, 0.0, 1.0]);
        let moved = o.clone().with_offset([1.0, 2.0, 3.0]);
        assert_eq!(moved.offset(), [1.0, 2.0, 3.0]);
        assert!(matches!(moved, Origin::Point { .. }));
    }
}
