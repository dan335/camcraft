//! What the snapshot harness asks of the app, to drive it into the states it
//! pictures: runs, picks, the playhead, the stock shown, prompts and windows.

use std::path::{Path, PathBuf};

use eframe::egui;

use super::App;
use crate::document::Document;
use crate::file_dialog::FileRequest;
use crate::playback;
use crate::tools::manager::ToolManager;
use crate::tools::picker::ToolPicker;
use crate::ui::measure::Measure;

impl App {
    /// Run the showing setup, as Generate does. Only the snapshot harness asks.
    pub fn start_run(&mut self, ctx: &egui::Context) {
        if let Some(name) = self.doc().and_then(|d| d.active.clone()) {
            self.generate(ctx, &name, None);
        }
    }

    /// Whether anything is running. Only the snapshot harness asks.
    pub fn busy(&self) -> Option<&'static str> {
        (!self.worker.idle()).then_some("working")
    }

    /// The showing setup's last run, if any. Only the snapshot harness asks.
    pub fn outcome(&self) -> Option<&camcraft_session::Ran> {
        let d = self.doc()?;
        Some(&d.setups.get(d.active.as_ref()?)?.ran.as_ref()?.ran)
    }

    /// The showing setup's state. Only the snapshot harness asks.
    fn state_for_snapshot(&mut self) -> Option<&mut crate::document::SetupState> {
        self.active_state_mut()
    }

    /// The showing job. Only the snapshot harness asks.
    pub fn doc_for_snapshot(&mut self) -> Option<&mut Document> {
        self.doc_mut()
    }

    /// Select operation `op` of the showing setup, make its boundary a selection,
    /// and click the longest edge along the bottom of its first model into it, as
    /// a click in the view does. Whether anything was picked. Only the snapshot
    /// harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn pick_bottom_loop_for_snapshot(&mut self, op: usize) -> bool {
        use crate::picking::slots::Slot;
        let Some(doc) = self.docs.get_mut(self.active) else {
            return false;
        };
        let Some(setup) = doc.active.clone() else {
            return false;
        };
        let Some(model) = doc.setup(&setup).and_then(|s| s.models.first().cloned()) else {
            return false;
        };
        let Some((loaded, _)) = doc.models.get(&model).and_then(|s| s.loaded()) else {
            return false;
        };
        let body = &loaded.body;
        let Some(bottom) = body.mesh().bounds().map(|b| b.min.z) else {
            return false;
        };
        let t = body.topology();
        let Some(edge) = (0..t.edges().len())
            .filter_map(|e| u32::try_from(e).ok())
            .filter(|&e| {
                t.edge_points(body.mesh(), e)
                    .all(|p| (p.z - bottom).abs() < 1e-6)
            })
            .max_by(|&a, &b| {
                let len = |e| t.edge(e).map_or(0.0, |e| e.length);
                len(a).total_cmp(&len(b))
            })
        else {
            return false;
        };
        doc.selection = crate::document::Selection::Operation(setup.clone(), op);
        if let Some(o) = doc.operation_mut(&setup, op) {
            cut_selection(o);
        }
        self.picking.arm(doc.id, Slot::Edges { setup, op });
        self.picking.click_edge_for_snapshot(doc, &model, edge)
    }

    /// Stop picking. Only the snapshot harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn picking_disarm_for_snapshot(&mut self) {
        self.picking.disarm();
    }

    /// Select operation `op` of the showing setup and pick faces for it in the
    /// view: the two largest that face up. How many faces the model has. Only the
    /// snapshot harness asks.
    pub fn pick_faces_for_snapshot(&mut self, op: usize) -> usize {
        use camcraft_core::job::FeatureRef;
        use camcraft_geom::topology::FaceKind;
        let Some(doc) = self.docs.get_mut(self.active) else {
            return 0;
        };
        let (Some(setup), id) = (doc.active.clone(), doc.id) else {
            return 0;
        };
        let Some(model) = doc.setup(&setup).and_then(|s| s.models.first().cloned()) else {
            return 0;
        };
        let Some((loaded, _)) = doc.models.get(&model).and_then(|s| s.loaded()) else {
            return 0;
        };
        let t = loaded.body.topology();
        let mut up: Vec<(u32, f64, [f64; 3])> = t
            .faces()
            .iter()
            .enumerate()
            .filter_map(|(i, f)| match f.kind {
                FaceKind::Plane { normal, .. } if normal.z > 0.9 => {
                    let tri = loaded
                        .body
                        .mesh()
                        .triangle(*f.triangles.first()? as usize)?;
                    let at = (tri[0] + tri[1] + tri[2]) / 3.0;
                    let at = camcraft_core::setup::features::unplaced(&loaded.body, at);
                    Some((u32::try_from(i).ok()?, f.area, at))
                }
                _ => None,
            })
            .collect();
        up.sort_by(|a, b| b.1.total_cmp(&a.1));
        let faces = t.faces().len();
        doc.selection = crate::document::Selection::Operation(setup.clone(), op);
        let Some(o) = doc.operation_mut(&setup, op) else {
            return faces;
        };
        cut_selection(o);
        o.selection.faces = up
            .iter()
            .take(2)
            .map(|(i, _, at)| FeatureRef {
                model: model.clone(),
                index: *i,
                at: *at,
            })
            .collect();
        self.picking.arm(
            id,
            crate::picking::slots::Slot::Faces {
                setup: setup.clone(),
                op,
            },
        );
        faces
    }

    /// Select the showing setup and arm its work zero for picking; where a round
    /// rim is drawn, and the setup-frame centre it should snap to. Only the
    /// snapshot harness asks.
    pub fn arm_work_zero_for_snapshot(&mut self) -> Option<(egui::Pos2, glam::DVec3)> {
        let doc = self.docs.get_mut(self.active)?;
        let setup = doc.active.clone()?;
        doc.selection = crate::document::Selection::Setup(setup.clone());
        let id = doc.id;
        self.picking
            .arm(id, crate::picking::slots::Slot::WorkZero { setup });
        let eye = self.viewport.camera().eye().as_dvec3();
        let (rim, centre, rect) = self.picking.rim_for_snapshot(eye)?;
        let aspect = rect.width() / rect.height().max(1.0);
        let proj = crate::view::camera::Projector::new(
            self.viewport.camera().view_projection(aspect),
            rect,
        );
        Some((proj.screen(rim)?, centre))
    }

    /// Arm `slot` for picking in the showing job. Only the snapshot harness asks.
    pub fn arm_for_snapshot(&mut self, slot: crate::picking::slots::Slot) {
        if let Some(doc) = self.docs.get(self.active) {
            let id = doc.id;
            self.picking.arm(id, slot);
        }
    }

    /// Press Esc: stop picking. Only the snapshot harness asks.
    pub fn escape_for_snapshot(&mut self) {
        self.picking.escape();
    }

    /// Open the job dialog, as File › Open would. Only the snapshot harness asks.
    pub fn open_dialog_for_snapshot(&mut self) {
        self.dialog.open(FileRequest::OpenJobs, "");
    }

    /// Ask to close the showing job, as ⌘W would. Only the snapshot harness asks.
    pub fn close_for_snapshot(&mut self) {
        if let Some(id) = self.doc().map(|d| d.id) {
            self.close(id);
        }
    }

    /// Open the tool picker for an operation, or close it for `None`. Only the
    /// snapshot harness asks.
    pub fn pick_tool_for_snapshot(&mut self, target: Option<(&str, usize)>) {
        self.picker = target.and_then(|(setup, op)| {
            let d = self.doc()?;
            Some(ToolPicker::new(d.id, setup.into(), op, &d.job))
        });
    }

    /// Put the playhead part-way through operation `op`, and show only `roles`.
    /// Only the snapshot harness asks.
    pub fn set_view_for_snapshot(&mut self, op: usize, fraction: f64, roles: u32) {
        let Some(state) = self.state_for_snapshot() else {
            return;
        };
        if let Some(span) = state.ran.as_ref().and_then(|r| r.scene.ops.get(op)) {
            // A fraction of a count, for a test's choice of frame.
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss
            )]
            let into = (span.count as f64 * fraction) as usize;
            state.view.at = crate::view::scene::Playhead::at(span.first + into.min(span.count));
        }
        state.view.view.roles = roles;
    }

    /// Play back, the material cut as far as the playhead, with the toolpaths
    /// too if `paths`. Only the snapshot harness asks.
    pub fn playback_for_snapshot(&mut self, paths: bool) {
        if let Some(state) = self.state_for_snapshot() {
            crate::playback::set_mode(state, crate::document::ViewMode::Playback);
            state.view.view.roles = u32::MAX;
            state.view.paths_in_playback = paths;
        }
    }

    /// Back to setting up. Only the snapshot harness asks.
    pub fn setup_for_snapshot(&mut self) {
        if let Some(state) = self.state_for_snapshot() {
            crate::playback::set_mode(state, crate::document::ViewMode::Setup);
        }
    }

    /// Where the view cube turns to `region`, on the screen. Only the snapshot
    /// harness asks.
    pub fn cube_point_for_snapshot(&self, region: glam::IVec3) -> Option<egui::Pos2> {
        self.viewport.cube_point_for_snapshot(region)
    }

    /// The view's camera. Only the snapshot harness asks.
    pub fn camera_for_snapshot(&self) -> crate::view::camera::Camera {
        self.viewport.camera()
    }

    /// Bring the camera in on `at`. Only the snapshot harness asks.
    pub fn focus_for_snapshot(&mut self, at: glam::DVec3, radius: f32) {
        self.viewport.focus(at, radius);
    }

    /// The showing setup's playhead, and whether it is playing. Only the snapshot
    /// harness asks.
    pub fn playhead_for_snapshot(&mut self) -> Option<(crate::view::scene::Playhead, bool)> {
        let view = &self.state_for_snapshot()?.view;
        Some((view.at, view.transport.playing))
    }

    /// Whether the playback material stands at the playhead. Only the snapshot
    /// harness asks.
    pub fn live_caught_up_for_snapshot(&mut self) -> bool {
        self.state_for_snapshot()
            .and_then(|s| s.live.as_ref())
            .is_some_and(crate::stock::live::LiveStock::settled)
    }

    /// Go to the deepest steep descent, as clicking it would. Only the snapshot
    /// harness asks.
    pub fn jump_to_deepest_for_snapshot(&mut self) -> Option<f64> {
        let state = self.state_for_snapshot()?;
        let deepest = state
            .ran
            .as_ref()?
            .scene
            .steep
            .iter()
            .copied()
            .max_by(|a, b| a.drop.total_cmp(&b.drop))?;
        let at = playback::jump_to(state, deepest);
        self.viewport.focus(at, 12.0);
        Some(deepest.drop)
    }

    /// Make one of the showing setup's checks fail, as a rapid through the part
    /// would, then post every setup into `dir`: the HALT banner, and the prompt
    /// that asks before posting anyway, once the worker has rendered the
    /// programs. Only the snapshot harness asks.
    pub fn fail_a_check_and_post_for_snapshot(&mut self, dir: &Path) {
        use camcraft_session::checks::Level as CheckLevel;
        let id = self.doc().map(|d| d.id);
        if let Some(ran) = self.state_for_snapshot().and_then(|s| s.ran.as_mut()) {
            let len = ran
                .ran
                .result
                .operations
                .first()
                .map_or(0, |o| o.path.len());
            if let Some(f) = ran
                .ran
                .checks
                .groups
                .get_mut(1)
                .and_then(|g| g.findings.iter_mut().find(|f| f.code == "CC102"))
            {
                f.level = CheckLevel::Fail;
                f.message = "1 rapid(s) pass through the part".into();
                f.detail = "deepest 2.40 mm under the cutter".into();
                f.at_move = Some(len / 2);
            }
            ran.marks = crate::transport::marks(&ran.ran, &ran.scene);
        }
        if let Some(id) = id {
            // The worker only wakes the window with it; the harness steps it.
            let ctx = egui::Context::default();
            self.prepare_post(&ctx, id, None, dir);
            while !self.worker.idle() {
                std::thread::sleep(std::time::Duration::from_millis(10));
                for (id, task, reply) in self.worker.poll() {
                    self.accept(&ctx, id, task, reply);
                }
            }
        }
    }

    /// Use a library of two machines' folders, kept at `path`. Only the snapshot
    /// harness asks.
    pub fn library_for_snapshot(&mut self, path: PathBuf) {
        self.tools = ToolManager::with_library(path);
        self.tools.folders_for_snapshot();
    }

    /// Show lengths in `measure`, with the settings window open. Only the snapshot
    /// harness asks.
    pub fn settings_for_snapshot(&mut self, measure: Measure) {
        self.settings.measure = measure;
        self.settings_open = true;
    }
}

/// Keep `o` to its picks: a 2D operation's outline, or a 3D one's boundary.
fn cut_selection(o: &mut camcraft_core::job::Operation) {
    use camcraft_core::job::{Boundary, Outline};
    if o.strategy.uses().outline {
        o.outline = Outline::Selection;
    } else {
        o.boundary.kind = Boundary::Selection;
    }
}
