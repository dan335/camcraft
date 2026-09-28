//! Models: which reader a file gets, what its numbers mean, and where a job's
//! model files are.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use camcraft_core::job::{Job, LengthUnit, ModelDef, Transform};
use camcraft_geom::mesh::{Aabb, TriMesh};
use camcraft_geom::topology::{Body, FaceSource};
use camcraft_import::step::{FileCheck, load_step};
use camcraft_import::stl::{Units, load_stl};

use crate::{SessionError, fs};

/// A model, loaded, placed and measured.
#[derive(Debug, Clone)]
pub struct Model {
    /// The triangles, in millimetres, and their faces, edges and vertices, where
    /// the job places them.
    pub body: Body,
    /// Their bounds.
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
        Ok(Model {
            body,
            bounds,
            ..self.clone()
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
            let (body, report) = load_step(path, tessellation_tolerance(tolerance))?;
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

/// A job's models, loaded, by name.
pub type Models<'a> = BTreeMap<&'a str, &'a Model>;

/// Load every model the setups at `setups` use, each once however many use it,
/// found relative to `job_dir` at the job's tolerance, by name.
pub fn load_models(
    job: &Job,
    job_dir: &Path,
    setups: &[usize],
) -> Result<BTreeMap<String, Model>, SessionError> {
    let mut loaded = BTreeMap::new();
    for &i in setups {
        for m in job.models_for(i) {
            if !loaded.contains_key(&m.name) {
                loaded.insert(m.name.clone(), load_job_model(job_dir, m, job.tolerance)?);
            }
        }
    }
    Ok(loaded)
}

/// Loaded models as a run takes them.
pub fn models_of(loaded: &BTreeMap<String, Model>) -> Models<'_> {
    loaded.iter().map(|(n, m)| (n.as_str(), m)).collect()
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
