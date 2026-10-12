//! What is uploaded of the content the viewport is given: each piece kept under
//! the key it was made under, and made again only when that changes.

use camcraft_geom::mesh::TriMesh;
use eframe::egui_wgpu::RenderState;
use eframe::wgpu;
use glam::Vec3;
use wgpu::util::DeviceExt;

use super::content::{Content, Outline, OutlineKind, Plane, PlaneKind};
use super::gpu::Gpu;
use super::lines::{line_instances, outline_instances};
use super::v3;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
}

/// Unshared vertices with a face normal each, so every triangle shades flat. Flat
/// shading shows facets, which is honest about what the engine is cutting against.
fn flat_vertices(triangles: impl Iterator<Item = [Vec3; 3]>) -> Vec<Vertex> {
    let mut out = Vec::new();
    for [a, b, c] in triangles {
        let n = (b - a).cross(c - a).normalize_or_zero().to_array();
        for p in [a, b, c] {
            out.push(Vertex {
                position: p.to_array(),
                normal: n,
            });
        }
    }
    out
}

fn mesh_triangles(mesh: &TriMesh) -> impl Iterator<Item = [Vec3; 3]> + '_ {
    (0..mesh.len())
        .filter_map(|i| mesh.triangle(i))
        .map(|t| t.map(|p| Vec3::from(v3(p))))
}

/// A highlight on the GPU: its faces and its lines, either of which may be empty.
pub(super) struct HighlightGpu {
    key: u64,
    pub(super) faces: Option<(wgpu::Buffer, u32)>,
    pub(super) lines: Option<(wgpu::Buffer, u32)>,
}

pub(super) struct Uploaded {
    pub(super) part: Option<(u64, wgpu::Buffer, u32)>,
    pub(super) selection: Option<HighlightGpu>,
    pub(super) hover: Option<HighlightGpu>,
    pub(super) ghost: Option<HighlightGpu>,
    pub(super) scene: Option<SceneGpu>,
    pub(super) outline: Option<(u64, wgpu::Buffer, u32)>,
    /// The height planes as uploaded: each one's fill, and all their edges.
    pub(super) planes: Option<PlanesGpu>,
}

impl Uploaded {
    /// Nothing yet.
    pub(super) fn none() -> Self {
        Self {
            part: None,
            selection: None,
            hover: None,
            ghost: None,
            scene: None,
            outline: None,
            planes: None,
        }
    }
}

/// A run's toolpaths and cutters on the GPU, with the key they were made under.
pub(super) struct SceneGpu {
    key: u64,
    pub(super) moves: Option<(wgpu::Buffer, u32)>,
    /// How many lines the moves up to each one make.
    pub(super) ends: Vec<u32>,
    /// One per operation; `None` where the job named no tool that could be drawn.
    pub(super) cutters: Vec<Option<(wgpu::Buffer, u32)>>,
}

/// Height planes on the GPU, with the planes they were made from.
pub(super) struct PlanesGpu {
    from: Vec<Plane>,
    pub(super) fills: Vec<(PlaneKind, wgpu::Buffer)>,
    pub(super) lines: (wgpu::Buffer, u32),
}

/// `data` in a new vertex buffer, with how many items it holds.
fn vertex_buffer<T: bytemuck::Pod>(
    rs: &RenderState,
    label: &str,
    data: &[T],
) -> (wgpu::Buffer, u32) {
    let buffer = rs
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: bytemuck::cast_slice(data),
            usage: wgpu::BufferUsages::VERTEX,
        });
    (buffer, u32::try_from(data.len()).unwrap_or(u32::MAX))
}

/// As [`vertex_buffer`], or nothing for no `data`: a buffer may not be empty, and
/// an empty one would draw nothing anyway.
fn vertex_buffer_of<T: bytemuck::Pod>(
    rs: &RenderState,
    label: &str,
    data: &[T],
) -> Option<(wgpu::Buffer, u32)> {
    (!data.is_empty()).then(|| vertex_buffer(rs, label, data))
}

/// Keep `slot`, an upload made under a key, in step with `wanted`, the key it
/// should be made under now and what from: made again by `make` when the key has
/// changed, dropped when nothing is wanted, and otherwise left as it is.
fn keep_up<K: PartialEq, W, U>(
    slot: &mut Option<U>,
    key: impl Fn(&U) -> K,
    wanted: Option<(K, W)>,
    make: impl FnOnce(K, W) -> Option<U>,
) {
    match wanted {
        Some((k, w)) if slot.as_ref().map(key).as_ref() != Some(&k) => *slot = make(k, w),
        Some(_) => {}
        None => *slot = None,
    }
}

impl Gpu {
    /// Upload whatever changed since the last frame.
    pub(super) fn upload(&mut self, rs: &RenderState, content: &Content<'_>) {
        self.upload_stock(rs, content.stock.as_ref());

        keep_up(
            &mut self.up.part,
            |p| p.0,
            content.part,
            |key, mesh| {
                let v = flat_vertices(mesh_triangles(mesh));
                let (buffer, n) = vertex_buffer(rs, "camcraft part", &v);
                Some((key, buffer, n))
            },
        );

        for (slot, wanted) in [
            (&mut self.up.selection, content.selection),
            (&mut self.up.hover, content.hover),
            (&mut self.up.ghost, content.ghost),
        ] {
            keep_up(
                slot,
                |h| h.key,
                wanted,
                |key, h| {
                    let faces = flat_vertices(h.faces.iter().copied());
                    Some(HighlightGpu {
                        key,
                        faces: vertex_buffer_of(rs, "camcraft picked faces", &faces),
                        lines: vertex_buffer_of(
                            rs,
                            "camcraft picked lines",
                            &outline_instances(&h.lines),
                        ),
                    })
                },
            );
        }

        if content.planes.is_empty() {
            self.up.planes = None;
        } else if self
            .up
            .planes
            .as_ref()
            .is_none_or(|p| p.from != content.planes)
        {
            let fills = content
                .planes
                .iter()
                .map(|p| {
                    let [a, b, c, d] = p.corners().map(|c| Vec3::from(v3(c)));
                    let v = flat_vertices([[a, b, c], [a, c, d]].into_iter());
                    (p.kind, vertex_buffer(rs, "camcraft plane", &v).0)
                })
                .collect();
            let edges: Vec<Outline> = content
                .planes
                .iter()
                .flat_map(|p| {
                    let c = p.corners();
                    let next = c.into_iter().cycle().skip(1);
                    c.into_iter().zip(next).map(move |(a, b)| Outline {
                        a,
                        b,
                        kind: OutlineKind::Plane(p.kind),
                    })
                })
                .collect();
            self.up.planes = Some(PlanesGpu {
                from: content.planes.to_vec(),
                fills,
                lines: vertex_buffer(rs, "camcraft plane edges", &outline_instances(&edges)),
            });
        }

        keep_up(
            &mut self.up.outline,
            |o| o.0,
            content.outline,
            |key, outline| {
                let lines = outline_instances(outline);
                vertex_buffer_of(rs, "camcraft outline", &lines).map(|(buffer, n)| (key, buffer, n))
            },
        );

        keep_up(
            &mut self.up.scene,
            |s| s.key,
            content.scene,
            |key, scene| {
                let (lines, ends) = line_instances(scene);
                Some(SceneGpu {
                    key,
                    moves: vertex_buffer_of(rs, "camcraft moves", &lines),
                    ends,
                    cutters: scene
                        .ops
                        .iter()
                        .map(|op| {
                            let v = flat_vertices(op.cutter.iter().copied());
                            vertex_buffer_of(rs, "camcraft cutter", &v)
                        })
                        .collect(),
                })
            },
        );
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use camcraft_geom::synthetic;

    #[test]
    fn every_triangle_becomes_three_flat_shaded_vertices() {
        let mesh = synthetic::plane(0.0, 10.0);
        let v = flat_vertices(mesh_triangles(&mesh));
        assert_eq!(v.len(), mesh.len() * 3);
        for tri in v.chunks(3) {
            // Flat: the three corners share one normal, of unit length.
            assert!(tri.iter().all(|x| x.normal == tri[0].normal));
            let n = Vec3::from(tri[0].normal);
            assert!((n.length() - 1.0).abs() < 1e-5);
            // A plane at Z=0 faces straight up or straight down.
            assert!(n.z.abs() > 0.999);
        }
    }
}
