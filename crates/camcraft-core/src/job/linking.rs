//! How the tool moves between cuts: Fusion's Linking tab.

use serde::{Deserialize, Serialize};

use super::{Operation, is_default, yes};

/// How the tool moves between cuts.
///
/// What each setting does, and why its default is Fusion's, is in
/// `docs/decisions/0004-linking-and-entries.md`.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
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
    /// How far above the material the tool stops rapiding down and feeds, and up to
    /// which it feeds on the way out, in millimetres.
    #[serde(
        default = "default_safe_distance",
        skip_serializing_if = "is_default_safe_distance"
    )]
    pub safe_distance: f64,
    /// The longest link made without lifting clear, in millimetres: the
    /// strategy's own when not given ([`Operation::stay_down`], Fusion's
    /// Maximum Stay-Down Distance for it), zero to always lift.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stay_down: Option<f64>,
    /// The radius of the vertical arcs each pass is entered and left along, for a
    /// strategy that follows the part ([`Uses::lead_radius`](super::Uses::lead_radius)),
    /// in millimetres: a tenth of the tool's diameter when not given, zero for
    /// none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_radius: Option<f64>,
    /// How steeply a strategy that cuts level by level ramps into material, along
    /// a ramp or down a helix, in degrees from level
    /// ([`Uses::ramp`](super::Uses::ramp)).
    #[serde(
        default = "default_ramp_angle",
        skip_serializing_if = "is_default_ramp_angle"
    )]
    pub ramp_angle: f64,
    /// Feed along a ramp, in mm/min: 60% of the cutting feed when not given,
    /// as Fusion's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ramp_feed: Option<f64>,
    /// The leads a 2D profile enters and leaves each ring along, in the waste
    /// beside it: Fusion's lead-in and lead-out, on unless turned off, as
    /// Fusion's are. `None` for none, written `leads = false`.
    #[serde(
        default = "fusion_leads",
        with = "leads_setting",
        skip_serializing_if = "is_fusion_leads"
    )]
    pub leads: Option<Leads>,
    /// How high the tool goes between cuts that are not made staying down.
    #[serde(default, skip_serializing_if = "is_default")]
    pub retraction: Retraction,
    /// How a pass is entered where it starts in material, for a strategy that
    /// offers a choice ([`Operation::entry`]): down a helix when not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<Entry>,
    /// The diameter of a helical entry's circle, as the tool's centre goes, in
    /// millimetres: the strategy's own when not given
    /// ([`Operation::helix_diameter`]), smaller where there is no room for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub helix_diameter: Option<f64>,
    /// The most a helix or a ramp round a ring descends in one lap, in
    /// millimetres: Fusion's maximum ramp stepdown. The operation's stepdown
    /// when not given, or the tool's diameter without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ramp_stepdown: Option<f64>,
}

/// How high the tool goes between cuts, as Fusion's retraction policy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Retraction {
    /// Up only as far as clears the material under the way across, and the safe
    /// distance above it, as the stock stood before the operation; never above
    /// the retract height unless the material is.
    #[default]
    Minimum,
    /// Up to the retract height, or higher where the material under the way
    /// across is.
    Full,
}

/// How a pass that starts in material is entered.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Entry {
    /// Down a helix at the ramp angle: in the waste beside a profile's lead,
    /// or beside where a pocket's first ring starts.
    #[default]
    Helix,
    /// Straight down at the plunge feed, beside a profile's lead, as a 2D
    /// contour without a ramp does in Fusion.
    Plunge,
    /// Down along the pass itself, in the direction it cuts, at the ramp angle.
    Ramp,
}

impl Entry {
    /// Its name as a job file writes it.
    pub fn name(self) -> &'static str {
        match self {
            Entry::Helix => "helix",
            Entry::Plunge => "plunge",
            Entry::Ramp => "ramp",
        }
    }
}

/// How a profile comes onto a ring and off it, in the waste beside it, as
/// Fusion's lead-in and lead-out, the same both ways: down a vertical arc, along
/// a straight, and round a level arc onto the ring, tangent to it; and off it the
/// same, mirrored. Each is a tenth of the tool's diameter when not given, as
/// Fusion's.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Leads {
    /// The level arc's radius, in millimetres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<f64>,
    /// How far round the level arc turns, in degrees: Fusion's sweep angle.
    #[serde(
        default = "default_lead_sweep",
        skip_serializing_if = "is_default_lead_sweep"
    )]
    pub sweep: f64,
    /// The straight before the level arc in and after it out, in millimetres:
    /// Fusion's linear lead-in distance; zero for none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub straight: Option<f64>,
    /// The vertical arc's radius, in millimetres: Fusion's vertical lead-in
    /// radius; zero for none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vertical: Option<f64>,
}

impl Default for Leads {
    /// Fusion's: every size a tenth of the tool, a quarter turn.
    fn default() -> Self {
        Self {
            radius: None,
            sweep: default_lead_sweep(),
            straight: None,
            vertical: None,
        }
    }
}

/// Fusion's leads, which a profile has unless they are turned off.
fn fusion_leads() -> Option<Leads> {
    Some(Leads::default())
}

// Serde's `skip_serializing_if` hands the field by reference.
#[allow(clippy::ref_option)]
fn is_fusion_leads(v: &Option<Leads>) -> bool {
    *v == fusion_leads()
}

/// A profile's leads as a job file writes them: a table of their sizes, or
/// `false` for none (and `true` for Fusion's, though leaving them out says so).
mod leads_setting {
    use serde::de::{self, MapAccess, Visitor};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::Leads;

    // Serde's `with` hands the field by reference.
    #[allow(clippy::ref_option)]
    pub fn serialize<S: Serializer>(leads: &Option<Leads>, s: S) -> Result<S::Ok, S::Error> {
        match leads {
            None => s.serialize_bool(false),
            Some(l) => l.serialize(s),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Leads>, D::Error> {
        struct Setting;
        impl<'de> Visitor<'de> for Setting {
            type Value = Option<Leads>;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a table of lead sizes, or false for none")
            }

            fn visit_bool<E: de::Error>(self, on: bool) -> Result<Self::Value, E> {
                Ok(on.then(Leads::default))
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
                Leads::deserialize(de::value::MapAccessDeserializer::new(map)).map(Some)
            }
        }
        d.deserialize_any(Setting)
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_default_lead_sweep(v: &f64) -> bool {
    v.to_bits() == default_lead_sweep().to_bits()
}

impl Leads {
    /// Fusion's leads for a tool `diameter` across: a quarter turn, and a tenth
    /// of the diameter each.
    pub fn sizes(&self, diameter: f64) -> LeadSizes {
        let tenth = 0.1 * diameter;
        LeadSizes {
            radius: self.radius.unwrap_or(tenth),
            sweep: self.sweep,
            straight: self.straight.unwrap_or(tenth),
            vertical: self.vertical.unwrap_or(tenth),
        }
    }
}

/// A profile's leads, sized for its tool.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LeadSizes {
    /// The level arc's radius, in millimetres.
    pub radius: f64,
    /// How far round the level arc turns, in degrees.
    pub sweep: f64,
    /// The straight, in millimetres.
    pub straight: f64,
    /// The vertical arc's radius, in millimetres.
    pub vertical: f64,
}

fn default_lead_sweep() -> f64 {
    90.0
}

impl Default for Linking {
    fn default() -> Self {
        Self {
            retract_feed: default_retract_feed(),
            plunge_feed: default_plunge_feed(),
            reorder: true,
            allow_reverse: false,
            safe_distance: default_safe_distance(),
            stay_down: None,
            lead_radius: None,
            ramp_angle: default_ramp_angle(),
            ramp_feed: None,
            leads: fusion_leads(),
            retraction: Retraction::Minimum,
            entry: None,
            helix_diameter: None,
            ramp_stepdown: None,
        }
    }
}

/// The share of the cutting feed a ramp goes at unless told otherwise: Fusion's
/// program for the lego ramps at 90 in/min to its 150.
const RAMP_SHARE: f64 = 0.6;

fn default_safe_distance() -> f64 {
    2.0
}

// Serde's `skip_serializing_if` hands the field by reference.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_default_safe_distance(v: &f64) -> bool {
    v.to_bits() == default_safe_distance().to_bits()
}

fn default_ramp_angle() -> f64 {
    2.0
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_default_ramp_angle(v: &f64) -> bool {
    v.to_bits() == default_ramp_angle().to_bits()
}

fn default_retract_feed() -> f64 {
    1500.0
}

fn default_plunge_feed() -> f64 {
    300.0
}

// Each setting as given, or its strategy's default for the tool: the one place
// the defaults are, so the panel says what the run does.
impl Operation {
    /// The feed along a ramp or a helix, in mm/min: as given, or 60% of the
    /// cutting feed, as Fusion's program ramps.
    pub fn ramp_feed(&self) -> f64 {
        self.linking
            .ramp_feed
            .unwrap_or(RAMP_SHARE * self.passes.feed)
    }

    /// The longest link it makes without lifting clear, in millimetres, with a
    /// tool `diameter` across: as given, or its strategy's own
    /// ([`Uses::reach`](super::Uses::reach)). Zero, always lifting, for one
    /// that never stays down.
    pub fn stay_down(&self, diameter: f64) -> f64 {
        let uses = self.strategy.uses();
        if !uses.stay_down {
            return 0.0;
        }
        self.linking
            .stay_down
            .unwrap_or_else(|| uses.reach.of(diameter))
    }

    /// The radius of the vertical arcs each pass is entered and left along, in
    /// millimetres, with a tool `diameter` across: as given, or a tenth of the
    /// tool, for a strategy that has them
    /// ([`Uses::lead_radius`](super::Uses::lead_radius)); zero for one that has
    /// not.
    pub fn lead_radius(&self, diameter: f64) -> f64 {
        if !self.strategy.uses().lead_radius {
            return 0.0;
        }
        self.linking.lead_radius.unwrap_or(0.1 * diameter)
    }

    /// Its leads, sized for a tool `diameter` across, when they are on and its
    /// strategy has them ([`Uses::leads`](super::Uses::leads)).
    pub fn leads(&self, diameter: f64) -> Option<LeadSizes> {
        self.linking
            .leads
            .filter(|_| self.strategy.uses().leads)
            .map(|l| l.sizes(diameter))
    }

    /// How a pass that starts in material is entered, for a strategy that offers
    /// a choice ([`Uses::entries`](super::Uses::entries)): as given, or down a helix.
    pub fn entry(&self) -> Option<Entry> {
        let offered = self.strategy.uses().entries;
        (!offered.is_empty()).then_some(self.linking.entry.unwrap_or_default())
    }

    /// The diameter of a helical entry's circle, as the tool's centre goes, in
    /// millimetres, with a tool `diameter` across whose corners are of radius
    /// `corner`: as given, or its strategy's own
    /// ([`Uses::helix`](super::Uses::helix)). Where there is no room for it,
    /// the helix is made smaller, down to [`Operation::least_helix_diameter`].
    pub fn helix_diameter(&self, diameter: f64, corner: f64) -> f64 {
        self.linking
            .helix_diameter
            .unwrap_or_else(|| self.strategy.uses().helix.diameter(diameter, corner))
    }

    /// The smallest a helical entry is made where there is no room for
    /// [`Operation::helix_diameter`]'s, for the same tool.
    pub fn least_helix_diameter(&self, diameter: f64, corner: f64) -> f64 {
        let largest = self.helix_diameter(diameter, corner);
        self.strategy.uses().helix.least(largest, diameter, corner)
    }

    /// The most a helix or a ramp round a ring descends in one lap, in
    /// millimetres, with a tool `diameter` across: as given, or its stepdown, or
    /// the tool's diameter without one.
    pub fn ramp_stepdown(&self, diameter: f64) -> f64 {
        self.linking
            .ramp_stepdown
            .or(self.stepdown())
            .unwrap_or(diameter)
    }
}
