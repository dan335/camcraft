//! A single finishing operation described by its numbers: the command line's
//! `post`.

use std::collections::BTreeMap;

use camcraft_core::job::{
    Height, HeightRef, Heights, Job, Linking, ModelDef, Operation, Passes, Setup, StockDef,
    StockSpec, StockToLeave, StrategySpec, ToolKind, ToolSpec,
};

/// A single finishing operation, described by its numbers rather than a file.
#[derive(Debug, Clone, Copy)]
pub struct FinishSpec {
    /// The strategy, with its own parameters.
    pub strategy: StrategySpec,
    /// Cutter shape.
    pub tool: ToolKind,
    /// A bullnose cutter's corner radius, in millimetres.
    pub corner: f64,
    /// Cutter diameter, in millimetres.
    pub diameter: f64,
    /// Distance between passes, in millimetres.
    pub stepover: f64,
    /// Distance between levels, in millimetres, for a strategy that cuts in
    /// levels; its own default unless given.
    pub stepdown: Option<f64>,
    /// Chordal tolerance, in millimetres.
    pub tolerance: f64,
    /// Stock to leave perpendicular to the tool axis, in millimetres.
    pub stock_radial: f64,
    /// Stock to leave along the tool axis, in millimetres.
    pub stock_axial: f64,
    /// Largest Z drop that may be cut, in millimetres.
    pub max_cut_step: f64,
    /// How far above the model's top the tool arrives and leaves, in
    /// millimetres.
    pub clearance: f64,
    /// Cutting feed, in mm/min.
    pub feed: f64,
    /// Work offset, 54 for G54.
    pub wcs: u8,
    /// Spindle speed.
    pub rpm: u32,
    /// Tool number.
    pub number: u32,
}

/// The one-operation job a [`FinishSpec`] describes, for `model`.
///
/// The block is the model's own bounding box. The tool arrives and leaves
/// `clearance` above the model's top, and between cuts lifts only clear of the
/// material. Retracts run at the cutting feed and plunges at a quarter of it.
/// Its one setup is called `finish`.
pub fn single_operation(spec: &FinishSpec, model: ModelDef) -> Job {
    const TOOL: &str = "cutter";
    let shape = match spec.tool {
        ToolKind::Ball => "ball",
        ToolKind::Flat => "flat",
        ToolKind::Bullnose => "bullnose",
        ToolKind::Cone => "cone",
        ToolKind::Drill => "drill",
    };
    let tool = ToolSpec {
        kind: spec.tool,
        diameter: spec.diameter,
        corner: if spec.tool == ToolKind::Bullnose {
            spec.corner
        } else {
            0.0
        },
        number: spec.number,
        rpm: spec.rpm,
        comment: format!("{}mm {shape}", spec.diameter),
        ..ToolSpec::default()
    };
    let mut setup = Setup::new("finish");
    setup.models = vec![model.name.clone()];
    setup.stocks = vec!["block".into()];
    setup.wcs = spec.wcs;
    setup.operations = vec![Operation {
        name: "finish".into(),
        strategy: spec.strategy,
        tool: Some(TOOL.into()),
        preset: None,
        selection: Default::default(),
        boundary: Default::default(),
        outline: Default::default(),
        heights: Heights {
            clearance: Height {
                reference: HeightRef::ModelTop,
                offset: spec.clearance,
            },
            ..Heights::default()
        },
        passes: Passes {
            stepover: spec.stepover,
            stepdown: spec.stepdown.or(spec.strategy.default_stepdown()),
            feed: spec.feed,
            stock_to_leave: StockToLeave {
                radial: spec.stock_radial,
                axial: spec.stock_axial,
            },
            max_cut_step: spec.max_cut_step,
            rpm: None,
            tabs: None,
        },
        linking: Linking {
            retract_feed: spec.feed,
            plunge_feed: spec.feed / 4.0,
            ..Linking::default()
        },
        enabled: true,
    }];
    Job {
        tolerance: spec.tolerance,
        tools: BTreeMap::from([(TOOL.to_string(), tool)]),
        models: vec![model],
        stocks: vec![StockDef {
            name: "block".into(),
            spec: StockSpec::AroundModels {
                models: Vec::new(),
                margin: [0.0; 3],
                below: 0.0,
            },
        }],
        setups: vec![setup],
        ..Job::default()
    }
}
