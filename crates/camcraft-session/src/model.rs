//! Models: which reader a file gets, what its numbers mean, and where a job's
//! model files are.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use camcraft_core::arrange::{Arrangement, Laid, Outcome, Outlines, SheetLayout, arrange};
use camcraft_core::control::Watch;
use camcraft_core::job::{Job, LengthUnit, ModelDef, Transform};
use camcraft_core::run::RunError;
use camcraft_core::setup::Bodies;
use camcraft_geom::mesh::{Aabb, TriMesh};
use camcraft_geom::topology::{Body, FaceSource};
use camcraft_import::step::{FileCheck, StepError, load_step};
use camcraft_import::stl::{Units, load_stl};

use crate::{SessionError, fs};

/// A model, loaded, placed and measured.
///
/// Where its copies stand is not the model's: a model a sheet arranges stands
/// where the job's [`Arrangement`] has it, which changes with the sheet while
/// the body stays as it is.
#[derive(Debug, Clone)]
pub struct Model {
    /// The triangles, in millimetres, and their faces, edges and vertices, where
    /// the job places them: one body, however many copies of it stand.
    pub body: Body,
    /// The body's bounds, as the job places it.
    pub bounds: Aabb,
    /// The body where its file put it, to place it again from.
    pub file: Arc<Body>,
    /// Where it came from.
    pub path: String,
    /// What the model's numbers meant, as a person would say it.
    pub units: String,
    /// What loading did, in order, for a log.
    pub notes: Vec<String>,
    /// Anything about the import worth a person's attention.
    pub warnings: Vec<String>,
}

impl Model {
    /// The triangles, in millimetres.
    pub fn mesh(&self) -> &TriMesh {
        self.body.mesh()
    }

    /// The model placed by `transform` instead, without reading its file again.
    pub fn placed(&self, transform: &Transform) -> Result<Model, SessionError> {
        let body = self.file.placed(transform.matrix())?;
        let bounds = body.mesh().bounds().ok_or(SessionError::EmptyModel)?;
        // Field by field: cloning the whole model would copy the body only to
        // throw it away.
        Ok(Model {
            body,
            bounds,
            file: Arc::clone(&self.file),
            path: self.path.clone(),
            units: self.units.clone(),
            notes: self.notes.clone(),
            warnings: self.warnings.clone(),
        })
    }
}

/// A label for units, as a person would write them.
fn units_label(units: Units) -> &'static str {
    match units {
        Units::Millimetres => "mm",
        Units::Inches => "inch",
    }
}

/// Tessellation tolerance for a B-rep, from the machining tolerance.
///
/// The simulator verifies against this mesh, so the gap between it and the true
/// surface is error nothing downstream can see. A quarter keeps it a small part of
/// the budget without making tessellation the slow step.
fn tessellation_tolerance(tolerance: f64) -> f64 {
    tolerance / 4.0
}

/// The machining tolerance whose tessellation tolerance is `tessellation`: the
/// inverse of [`tessellation_tolerance`].
fn machining_tolerance(tessellation: f64) -> f64 {
    tessellation * 4.0
}

/// `x`, rounded up at its first significant figure below one, or to a whole
/// number from one: 0.0291 to 0.03. Float noise just past a step does not round
/// it up a whole step: 0.03 stays 0.03.
fn round_up(x: f64) -> f64 {
    if !(x.is_finite() && x > 0.0) {
        return x;
    }
    let mut scale = 1.0;
    while x * scale < 1.0 {
        scale *= 10.0;
    }
    ((x * scale) - 1e-9).ceil() / scale
}

/// Read a model, choosing the reader by extension.
///
/// An STL carries no units, so `units` is required for one. A STEP file states its
/// own, and `units`, if given, must agree with it. `tolerance` is the job's
/// machining tolerance; a STEP file is tessellated to a quarter of it.
pub fn load_model(
    path: &Path,
    units: Option<Units>,
    tolerance: f64,
) -> Result<Model, SessionError> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let mut notes = Vec::new();
    let mut warnings = Vec::new();
    let (body, label) = match ext.as_str() {
        "stl" => {
            let units = units.ok_or(SessionError::StlNeedsUnits)?;
            let (mesh, import) = load_stl(path, units)?;
            if import.degenerate > 0 {
                warnings.push(format!(
                    "{} degenerate triangle(s) in the model",
                    import.degenerate
                ));
            }
            notes.push(format!(
                "loaded {} triangles, {} vertices after welding",
                import.triangles, import.vertices
            ));
            // An STL file has no faces; they are found from the triangles.
            (Body::derived(mesh), units_label(units).to_string())
        }
        "step" | "stp" => {
            let started = std::time::Instant::now();
            let (body, report) =
                load_step(path, tessellation_tolerance(tolerance)).map_err(|e| match e {
                    StepError::FinerThanFile { edges, .. } => SessionError::FinerThanFile {
                        least: round_up(machining_tolerance(edges)),
                        source: e,
                    },
                    e => e.into(),
                })?;
            if let Some(said) = units
                && (said.to_mm() - report.unit.mm).abs() > 1e-9 * said.to_mm()
            {
                return Err(SessionError::UnitsDisagree {
                    said: units_label(said),
                    file: report.unit.name,
                });
            }
            if report.degenerate > 0 {
                warnings.push(format!(
                    "{} degenerate triangle(s) from tessellation",
                    report.degenerate
                ));
            }
            if let FileCheck::Skipped(why) = report.check {
                warnings.push(format!(
                    "the mesh was not checked against the file's geometry: {why}"
                ));
            }
            notes.push(format!(
                "read STEP in {}: {} face(s) all meshed, {} triangles at {} mm, checked: {}",
                report.unit.name,
                report.faces,
                report.triangles,
                report.tolerance,
                match report.check {
                    FileCheck::Passed => "yes",
                    FileCheck::Skipped(_) => "NO",
                },
            ));
            notes.push(format!(
                "tessellated in {:.1} s",
                started.elapsed().as_secs_f64()
            ));
            (body, format!("{} (from file)", report.unit.name))
        }
        _ => return Err(SessionError::UnknownFormat(path.display().to_string())),
    };
    let t = body.topology();
    notes.push(format!(
        "topology: {} face(s) {}, {} edge(s), {} vertices",
        t.faces().len(),
        match t.source() {
            FaceSource::File => "from the file",
            FaceSource::Derived => "found from the triangles",
        },
        t.edges().len(),
        t.vertices().len()
    ));

    let bounds = body.mesh().bounds().ok_or(SessionError::EmptyModel)?;
    let extent = bounds.extent();
    notes.push(format!(
        "model is {:.1} x {:.1} x {:.1} mm",
        extent.x, extent.y, extent.z
    ));
    Ok(Model {
        file: Arc::new(body.clone()),
        body,
        bounds,
        path: path.display().to_string(),
        units: label,
        notes,
        warnings,
    })
}

/// What a model's units, as a job states them, mean to the reader.
pub fn units_of(unit: Option<LengthUnit>) -> Option<Units> {
    unit.map(|u| match u {
        LengthUnit::Mm => Units::Millimetres,
        LengthUnit::Inch => Units::Inches,
    })
}

/// Load one of a job's models, found relative to `job_dir`, at the job's
/// tolerance.
pub fn load_job_model(
    job_dir: &Path,
    model: &ModelDef,
    tolerance: f64,
) -> Result<Model, SessionError> {
    let path = fs::resolve(job_dir, &model.path);
    if !path.exists() {
        return Err(SessionError::ModelMissing {
            name: model.name.clone(),
            path: path.display().to_string(),
        });
    }
    load_model(&path, units_of(model.units), tolerance)
        .and_then(|m| {
            if model.transform == Transform::default() {
                Ok(m)
            } else {
                m.placed(&model.transform)
            }
        })
        .map_err(|e| SessionError::InModel {
            name: model.name.clone(),
            source: Box::new(e),
        })
}

/// A job's models, loaded, by name, and where the job's sheets stand them: what
/// a run takes besides the job.
///
/// Collected from name and model pairs, the job has no sheets, and each model
/// stands once, where it is.
#[derive(Debug, Clone)]
pub struct Models<'a> {
    loaded: BTreeMap<&'a str, &'a Model>,
    arrangement: &'a Arrangement,
}

impl<'a> Models<'a> {
    /// The models `loaded`, by name, standing where `arrangement` has them.
    pub fn arranged(
        loaded: impl IntoIterator<Item = (&'a str, &'a Model)>,
        arrangement: &'a Arrangement,
    ) -> Self {
        Self {
            loaded: loaded.into_iter().collect(),
            arrangement,
        }
    }

    /// Model `name`, if it is loaded.
    pub fn get(&self, name: &str) -> Option<&'a Model> {
        self.loaded.get(name).copied()
    }

    /// Where the job's sheets stand their models.
    pub fn arrangement(&self) -> &'a Arrangement {
        self.arrangement
    }

    /// The models as the engine takes them: each its body and where its
    /// copies stand.
    pub(crate) fn bodies(&self) -> Bodies<'a> {
        Bodies::arranged(
            self.loaded.iter().map(|(&n, &m)| (n, &m.body)),
            self.arrangement,
        )
    }
}

impl<'a> FromIterator<(&'a str, &'a Model)> for Models<'a> {
    /// Each model standing once, where it is.
    fn from_iter<I: IntoIterator<Item = (&'a str, &'a Model)>>(models: I) -> Self {
        Self::arranged(models, Arrangement::none())
    }
}

/// A job's models loaded, by name, and its sheets arranged with them.
#[derive(Debug, Clone)]
pub struct Loaded {
    /// Each model, by name.
    pub models: BTreeMap<String, Model>,
    /// Where the job's sheets stand their parts.
    pub arrangement: Arrangement,
}

/// Load every model the setups at `setups` use, each once however many use it,
/// found relative to `job_dir` at the job's tolerance, by name; then arrange
/// the job's sheets with them. A sheet those setups use that cannot be
/// arranged is an error, named.
pub fn load_models(job: &Job, job_dir: &Path, setups: &[usize]) -> Result<Loaded, SessionError> {
    let mut models = BTreeMap::new();
    for &i in setups {
        for m in job.models_for(i) {
            if !models.contains_key(&m.name) {
                models.insert(m.name.clone(), load_job_model(job_dir, m, job.tolerance)?);
            }
        }
    }
    let bodies: BTreeMap<&str, &Body> = models.iter().map(|(n, m)| (n.as_str(), &m.body)).collect();
    let arrangement = arrange(job, &bodies, &mut Outlines::default(), &Watch::default())
        .map_err(|_| SessionError::Run(RunError::Stopped))?;
    let used: Vec<&str> = setups
        .iter()
        .filter_map(|&i| job.setups.get(i))
        .flat_map(|s| s.stocks.iter().map(String::as_str))
        .collect();
    for sheet in arrangement.sheets() {
        if let Laid::Failed(e) = &sheet.laid
            && used.contains(&sheet.stock.as_str())
        {
            return Err(SessionError::Arrange {
                stock: sheet.stock.clone(),
                source: e.clone(),
            });
        }
    }
    Ok(Loaded {
        models,
        arrangement,
    })
}

/// Loaded models as a run takes them.
pub fn models_of(loaded: &Loaded) -> Models<'_> {
    Models::arranged(
        loaded.models.iter().map(|(n, m)| (n.as_str(), m)),
        &loaded.arrangement,
    )
}

/// What became of each part of sheet `sheet`, a line each, for a log: where
/// it stands, or why it was left out, and how much of the sheet they use.
pub fn sheet_notes(sheet: &SheetLayout) -> Vec<String> {
    let name = &sheet.stock;
    let (parts, used, length) = match &sheet.laid {
        Laid::Done {
            parts,
            used,
            length,
        } => (parts, used, length),
        Laid::Waiting(missing) => {
            return vec![format!(
                "sheet '{name}': not arranged, waiting for {}",
                quoted(missing.iter().map(String::as_str))
            )];
        }
        Laid::Failed(e) => return vec![format!("sheet '{name}': cannot be arranged: {e}")],
    };
    let (placed, copies) = sheet.placed();
    let mut out = vec![format!(
        "sheet '{name}': {placed} of {copies} part(s) placed, {used:.1} of {length:.1} mm used"
    )];
    for p in parts {
        // A part of one copy is named alone; of several, each copy by its
        // place among them.
        let n = p.copies.len();
        for (k, c) in p.copies.iter().enumerate() {
            let which = match n {
                1 => format!("'{}'", p.model),
                _ => format!("'{}' copy {} of {n}", p.model, k + 1),
            };
            out.push(match c.outcome {
                Outcome::Placed(s) => format!(
                    "sheet '{name}': {which} at ({:.3}, {:.3}, {:.3}), turned {}°",
                    s.offset.x,
                    s.offset.y,
                    s.offset.z,
                    s.turn.degrees()
                ),
                Outcome::TooLarge => format!(
                    "sheet '{name}': {which} left out: it does not fit inside the border at any \
                     turn it may take"
                ),
                Outcome::NoRoom => {
                    format!("sheet '{name}': {which} left out: there is no room left for it")
                }
            });
        }
    }
    out
}

/// `names`, each in quotes, as a list in a sentence: 'a', 'b' and 'c'.
pub(crate) fn quoted<'a>(names: impl IntoIterator<Item = &'a str>) -> String {
    let all: Vec<String> = names.into_iter().map(|n| format!("'{n}'")).collect();
    match all.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => all.concat(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::test_support::fixture;

    #[test]
    fn the_reader_is_chosen_by_extension() {
        let err = load_model(Path::new("part.obj"), Some(Units::Millimetres), 0.05);
        assert!(matches!(err, Err(SessionError::UnknownFormat(_))));
    }

    #[test]
    fn an_stl_without_units_is_refused_before_it_is_read() {
        // The file does not exist: refusing must not depend on reading it.
        let err = load_model(Path::new("does-not-exist.stl"), None, 0.05);
        assert!(matches!(err, Err(SessionError::StlNeedsUnits)));
    }

    #[test]
    fn step_units_that_contradict_the_file_are_refused() {
        let err = load_model(&fixture("hinge_jig.step"), Some(Units::Millimetres), 0.05);
        match err {
            Err(SessionError::UnitsDisagree { said, file }) => {
                assert_eq!(said, "mm");
                assert_eq!(file, "inch");
            }
            other => panic!("expected a disagreement, got {other:?}"),
        }
    }

    /// Its edges stand 0.006 mm off the round they bound, which a machining
    /// tolerance of 0.01 meshes finer than: the message names the tolerance that
    /// reads it, 0.006 at a quarter, rounded up.
    #[test]
    fn a_tolerance_finer_than_the_file_names_one_that_reads_it() {
        let err = load_model(&fixture("round_edge_loose_fit.step"), None, 0.01);
        let Err(SessionError::FinerThanFile { least, .. }) = &err else {
            panic!("expected the file to be too coarse, got {:?}", err.err());
        };
        assert!((least - 0.03).abs() < 1e-12, "{least}");
        let message = err.err().unwrap().to_string();
        assert!(
            message.ends_with("machining tolerance of 0.03 mm or coarser"),
            "{message}"
        );
    }

    #[test]
    fn round_up_keeps_one_figure() {
        for (x, want) in [
            (0.0291, 0.03),
            (0.024, 0.03),
            (0.03, 0.03),
            (0.2, 0.2),
            (1.2, 2.0),
        ] {
            assert!((round_up(x) - want).abs() < 1e-12, "{x}: {}", round_up(x));
        }
    }

    #[test]
    fn step_units_that_agree_are_accepted_and_the_file_decides() {
        let m = load_model(&fixture("hinge_jig.step"), Some(Units::Inches), 0.05).unwrap();
        assert_eq!(m.units, "inch (from file)");
        let m = load_model(&fixture("hinge_jig.step"), None, 0.05).unwrap();
        assert!((m.bounds.extent().x - 25.4).abs() < 0.1);
        assert!(m.notes.iter().any(|n| n.starts_with("read STEP in inch")));
    }

    /// A model is loaded where the job places it, and keeps where its file put it
    /// to be placed again from.
    #[test]
    fn a_model_loads_where_it_is_placed() {
        let mut def = ModelDef {
            name: "hinge".into(),
            path: fixture("hinge_jig.step").display().to_string(),
            units: None,
            transform: Transform::default(),
        };
        let at_file = load_job_model(Path::new("/"), &def, 0.05).unwrap();
        def.transform = Transform {
            position: [100.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 90.0],
            scale: [2.0; 3],
        };
        let placed = load_job_model(Path::new("/"), &def, 0.05).unwrap();
        let (a, b) = (at_file.bounds.extent(), placed.bounds.extent());
        assert!(
            (b.x - 2.0 * a.y).abs() < 1e-6,
            "turned and doubled: {a} {b}"
        );
        assert!(
            (b.y - 2.0 * a.x).abs() < 1e-6,
            "turned and doubled: {a} {b}"
        );
        assert!((b.z - 2.0 * a.z).abs() < 1e-6, "doubled: {a} {b}");
        assert_eq!(*placed.file, at_file.body, "the file's body is kept");
        let back = placed.placed(&Transform::default()).unwrap();
        assert!(back.bounds.min.distance(at_file.bounds.min) < 1e-9);
        assert!(back.bounds.max.distance(at_file.bounds.max) < 1e-9);
    }

    /// A model that is not where the job says is named, with where it was looked
    /// for.
    #[test]
    fn a_missing_model_is_named() {
        let m = ModelDef {
            name: "gone".into(),
            path: "nowhere/part.step".into(),
            units: None,
            transform: Default::default(),
        };
        match load_job_model(Path::new("/tmp"), &m, 0.05) {
            Err(SessionError::ModelMissing { name, path }) => {
                assert_eq!(name, "gone");
                assert!(path.ends_with("nowhere/part.step"), "{path}");
            }
            other => panic!("expected a missing model, got {other:?}"),
        }
    }
}
