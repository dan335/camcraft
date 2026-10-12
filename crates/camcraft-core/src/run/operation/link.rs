//! How an operation is linked: its linking settings as the linker takes them,
//! and the material and part the linker measures its links against.

use super::*;

/// How `work`'s operation links: its own settings, with the strategy's defaults
/// where it gives none, as Fusion's.
pub(super) fn link_params(work: &Work<'_>, clearance_z: f64) -> LinkParams {
    let (op, linking) = (work.op, &work.op.linking);
    let diameter = work.spec.diameter;
    // A profile's vertical lead is part of its leads; a finishing pass's is its
    // own setting.
    let leads = op.leads(diameter);
    let uses = op.strategy.uses();
    let vertical = if uses.lead_radius {
        op.lead_radius(diameter)
    } else {
        leads.map_or(0.0, |l| l.vertical)
    };
    let (stay_down, lead_radius, ramp) = match uses.links {
        // Adaptive clearing stays down itself, at the level, only where it has
        // cleared the way, and goes down its own helixes: a link it leaves is
        // one to lift for, into what it has cleared, straight down.
        Links::Own => (StayDown::Never, 0.0, None),
        Links::OverPart => (
            StayDown::OverPart {
                max: op.stay_down(diameter),
            },
            vertical,
            None,
        ),
        Links::Level => (
            StayDown::Level {
                max: op.stay_down(diameter),
            },
            vertical,
            uses.ramp.then(|| Ramp {
                angle: linking.ramp_angle.to_radians(),
                // A ramp moves mostly sideways, so it goes nearer the cutting
                // feed than a straight plunge's.
                feed: op.ramp_feed(),
            }),
        ),
    };
    let entry = match op.entry() {
        Some(Entry::Helix) => EntryKind::Helix,
        Some(Entry::Plunge) => EntryKind::Plunge,
        // A profile's ramp is its own path, a spiral down each ring
        // (`planar_paths`): the linker only brings the tool onto its lead.
        Some(Entry::Ramp) => EntryKind::Plunge,
        None => EntryKind::Ramp,
    };
    LinkParams {
        retract_feed: linking.retract_feed,
        plunge_feed: linking.plunge_feed,
        tool_radius: work.tool.cutter_radius(),
        safe_distance: linking.safe_distance,
        stay_down,
        lead_radius,
        ramp,
        entry,
        helix_radius: op.helix_diameter(diameter) / 2.0,
        // A helix climbs as the cut does.
        helix_ccw: work.kept_on_right(),
        ramp_stepdown: Some(op.ramp_stepdown(diameter)),
        lead_in_material: leads.is_some(),
        max_cut_step: op.passes.max_cut_step,
        tolerance: work.job.tolerance,
        end_z: Some(clearance_z),
        start_z: Some(clearance_z),
    }
}

/// The operation's stock, cut as its moves are linked: what every link decision
/// is measured against.
pub(super) struct Cutting<'a> {
    pub(super) stock: &'a mut Stock,
    pub(super) tool: &'a EffectiveTool,
    pub(super) rapid_contacts: u64,
    pub(super) deepest: f64,
}

impl Material for Cutting<'_> {
    fn highest(&self, from: DVec2, to: DVec2, radius: f64) -> Option<f64> {
        // The simulator's own disc, down to its margin, so a rapid stopped above
        // this is one the simulator agrees touches nothing.
        highest_under(self.stock.field(), from, to, radius * (1.0 + 1e-9)).top
    }

    fn cut(&mut self, moves: &[Move]) {
        let swept = self.stock.sweep(&sim_moves_of(moves), self.tool);
        self.rapid_contacts += swept.rapid_contacts;
        self.deepest = self.deepest.max(swept.deepest_rapid_contact_mm);
    }
}

/// The part, as the operation's tool meets it.
pub(super) struct OnPart<'a> {
    cutter: &'a DropCutter<'a>,
    tool: &'a EffectiveTool,
    /// The tool a tolerance narrower: where one only grazing the part rests.
    slim: EffectiveTool,
    tolerance: f64,
}

impl<'a> OnPart<'a> {
    pub(super) fn new(cutter: &'a DropCutter<'a>, tool: &'a EffectiveTool, tolerance: f64) -> Self {
        let a = tool.allowance();
        let slim = Allowance::new(a.radial() - tolerance, a.axial())
            .ok()
            .and_then(|a| EffectiveTool::of(tool.base(), a).ok())
            .unwrap_or(*tool);
        Self {
            cutter,
            tool,
            slim,
            tolerance,
        }
    }
}

impl Surface for OnPart<'_> {
    fn at(&self, p: DVec2) -> Option<f64> {
        self.cutter.drop_at(self.tool, p.x, p.y)
    }

    fn along(&self, from: DVec2, to: DVec2) -> Vec<DVec3> {
        // Seeded at a quarter of the tool's width: nothing that narrow is passed over.
        let seed = (self.tool.max_radius() * 0.5).max(self.tolerance);
        follow(self.cutter, self.tool, from, to, self.tolerance, seed)
    }

    fn grazing_at(&self, p: DVec2) -> Option<f64> {
        self.cutter.drop_at(&self.slim, p.x, p.y)
    }

    fn grazing_along(&self, from: DVec2, to: DVec2) -> Vec<DVec3> {
        let seed = (self.slim.max_radius() * 0.5).max(self.tolerance);
        follow(self.cutter, &self.slim, from, to, self.tolerance, seed)
    }
}
