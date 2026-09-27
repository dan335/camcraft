//! A single finishing pass described by its numbers: the command line's `post`.

use std::collections::BTreeMap;

use camcraft_core::job::{
    Height, HeightRef, Heights, Job, Linking, ModelDef, Operation, Passes, Setup, StockDef,
    StockSpec, StockToLeave, StrategySpec, ToolKind, ToolSpec,
};

/// Cutter shape for a single finishing pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinishTool {
    /// Ball nose.
    Ball,
    /// Square end.
    Flat,
}

/// A single parallel finishing pass, described by its numbers rather than a file.
#[derive(Debug, Clone, Copy)]
pub struct FinishSpec {
    /// Cutter shape.
    pub tool: FinishTool,
    /// Cutter diameter, in millimetres.
    pub diameter: f64,
    /// Distance between passes, in millimetres.
    pub stepover: f64,
    /// Chordal tolerance, in millimetres.
    pub tolerance: f64,
    /// Stock to leave perpendicular to the tool axis, in millimetres.
    pub stock_radial: f64,
    /// Stock to leave along the tool axis, in millimetres.
    pub stock_axial: f64,
    /// Largest Z drop that may be cut, in millimetres.
    pub max_cut_step: f64,
    /// Traverse height above the material, in millimetres.
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
/// The block is the model's own bounding box, and traverses clear the material by
/// `clearance`. Retracts run at the cutting feed and plunges at a quarter of it.
/// Its one setup is called `finish`.
pub fn single_operation(spec: &FinishSpec, model: ModelDef) -> Job {
    const TOOL: &str = "cutter";
    let (kind, shape) = match spec.tool {
        FinishTool::Ball => (ToolKind::Ball, "ball"),
        FinishTool::Flat => (ToolKind::Flat, "flat"),
    };
    let tool = ToolSpec {
        kind,
        diameter: spec.diameter,
        corner: 0.0,
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
        strategy: StrategySpec::Parallel { angle: 0.0 },
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
            stepdown: None,
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
