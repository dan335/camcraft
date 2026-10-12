//! The material on the GPU: each tile's textures, grid and height pyramid, made
//! once for a stock's size, then written only where the cells changed.

use camcraft_geom::heightmap::narrow;
use camcraft_sim::stock::CellRect;
use eframe::egui_wgpu::RenderState;
use eframe::wgpu;

use super::content::{StockChange, StockShow};
use super::gpu::{Gpu, PipelineSpec, linear_rgba, pipeline, source};
use crate::stock::gpu_tiles::{self, Tile};
use crate::stock::pyramid::Pyramid;
use crate::stock::stock_ao::{self, WALL_CELLS};
use crate::stock::stock_view::{CLASSES, Class, ColorMode, NO_CELL, StockImage, cell_color};
use crate::ui::theme;

/// The material, drawn by tracing each pixel's ray through its heights.
///
/// The stock is a height per cell: from above, a column of material with a flat
/// top, which is exactly what 3-axis cutting leaves. So each column is drawn as
/// one: a ray that meets the side of a taller column meets a vertical wall, and
/// one that comes down onto a column meets its top. Nothing joins neighbouring
/// cells with a slope, so a wall the cutter left is drawn vertical, at any
/// resolution.
///
/// Only the stock's box is rasterised. Its far faces trace the ray from the eye
/// down [`crate::stock::pyramid`]'s levels of maxima (the height texture's mips),
/// skipping every square the ray passes over in one step, to the cell it first
/// meets; the near faces stand aside. The hit writes its own depth, so paths and
/// the cutter sit in front of it or behind it as they are. Tops are lit by the
/// surface's slope, found from the neighbouring cells that are not across a
/// wall, so a curved surface reads as smooth; the side of a column is lit as a
/// wall only where it stands more than [`WALL_CELLS`] cells tall.
pub(super) const STOCK_SHADER: &str = include_str!("shaders/stock.wgsl");

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GridUniforms {
    origin_pitch_floor: [f32; 4],
    size: [u32; 4],
    top_wall: [f32; 4],
    core: [u32; 4],
    palette: [[f32; 4]; 6],
    finish: [f32; 4],
}

/// Where the cells of `rect` start in an image `nx` cells wide with `bytes` per
/// cell, and how many bytes one of its rows spans.
fn rect_layout(rect: CellRect, nx: u32, bytes: u32) -> (u64, u32) {
    let offset = (u64::from(rect.y0) * u64::from(nx) + u64::from(rect.x0)) * u64::from(bytes);
    (offset, nx * bytes)
}

/// Write `rect` of mip `level` of `texture` from `data`, a grid `w` cells wide
/// with `bytes` per cell whose `from` rectangle holds it.
#[allow(clippy::too_many_arguments)]
fn write_texture(
    rs: &RenderState,
    texture: &wgpu::Texture,
    level: u32,
    data: &[u8],
    bytes: u32,
    w: u32,
    from: CellRect,
    rect: CellRect,
) {
    let (offset, bytes_per_row) = rect_layout(from, w, bytes);
    rs.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: level,
            origin: wgpu::Origin3d {
                x: rect.x0,
                y: rect.y0,
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        data,
        wgpu::TexelCopyBufferLayout {
            offset,
            bytes_per_row: Some(bytes_per_row),
            rows_per_image: Some(rect.height()),
        },
        wgpu::Extent3d {
            width: rect.width(),
            height: rect.height(),
            depth_or_array_layers: 1,
        },
    );
}

/// What to write of a tile: its cells (its own coordinates) and, for a whole
/// tile, their heights, codes and sky; and the changed cells of each pyramid
/// level.
struct TilePlan {
    cells: CellRect,
    whole: Option<(Vec<f32>, Vec<u8>, Vec<u8>)>,
    levels: Vec<CellRect>,
}

/// Bring a tile's pyramid up to date with `rects` of `image` (every cell, when
/// there are none), and say what to write. `None` when nothing of it changed.
fn plan_tile(
    tile: &mut TileGpu,
    rects: Option<&[CellRect]>,
    image: &StockImage,
    floor: f32,
) -> Option<TilePlan> {
    let t = tile.tile;
    let base = |x: u32, y: u32| {
        t.global(x, y, image.nx, image.ny)
            .and_then(|(gx, gy)| {
                image
                    .heights
                    .get(gy as usize * image.nx as usize + gx as usize)
                    .copied()
            })
            .unwrap_or(floor)
    };
    let Some(rects) = rects else {
        tile.pyramid = Pyramid::new(t.tex_w, t.tex_h, base);
        let cells = CellRect::all(t.w, t.h);
        return Some(TilePlan {
            cells,
            whole: Some((
                rows(t, cells, image, &image.heights, floor),
                rows(t, cells, image, &image.codes, NO_CELL),
                rows(t, cells, image, &image.ao, stock_ao::OPEN),
            )),
            levels: tile
                .pyramid
                .levels
                .iter()
                .map(crate::stock::pyramid::Level::all)
                .collect(),
        });
    };
    let local: Vec<CellRect> = rects.iter().filter_map(|r| t.local(*r)).collect();
    let cells = local.iter().copied().reduce(CellRect::union)?;
    // Each level's changed cells, as one rectangle: one write a level, however
    // many pieces changed, since each write has its own cost.
    let mut union: Vec<Option<CellRect>> = vec![None; tile.pyramid.levels.len()];
    for r in &local {
        for (u, c) in union.iter_mut().zip(tile.pyramid.update(*r, base)) {
            *u = CellRect::join(*u, Some(c));
        }
    }
    Some(TilePlan {
        cells,
        whole: None,
        levels: union.into_iter().flatten().collect(),
    })
}

/// Tile `t`'s cells `r` of `cells` (the image's, `nx` a row), row by row, with
/// `outside` where they are off the stock: whole rows copied at a time.
fn rows<T: Copy>(t: Tile, r: CellRect, image: &StockImage, cells: &[T], outside: T) -> Vec<T> {
    let (w, h) = (r.width() as usize, r.height() as usize);
    let mut out = vec![outside; w * h];
    // The stock columns this rectangle covers, and where they start in it.
    let gx0 = t.x0 + i64::from(r.x0);
    let lo = gx0.max(0);
    let hi = (gx0 + i64::from(r.width())).min(i64::from(image.nx));
    if lo >= hi {
        return out;
    }
    let (Ok(skip), Ok(lo), Ok(hi)) = (
        usize::try_from(lo - gx0),
        usize::try_from(lo),
        usize::try_from(hi),
    ) else {
        return out;
    };
    let nx = image.nx as usize;
    for (y, row) in (r.y0..=r.y1).zip(out.chunks_exact_mut(w)) {
        let Ok(gy) = usize::try_from(t.y0 + i64::from(y)) else {
            continue;
        };
        if gy >= image.ny as usize {
            continue;
        }
        if let (Some(src), Some(dst)) = (
            cells.get(gy * nx + lo..gy * nx + hi),
            row.get_mut(skip..skip + (hi - lo)),
        ) {
            dst.copy_from_slice(src);
        }
    }
    out
}

/// The stock shader's colours, linear: below the part, leftover faint and
/// strong, on the part, off the part, and the material. The same colours the
/// legend shows, from the same functions.
fn palette() -> [[f32; 4]; 6] {
    let faint = cell_color(ColorMode::Compare, Class::Leftover, 0.0);
    let strong = cell_color(ColorMode::Compare, Class::Leftover, f64::INFINITY);
    let [gouge, _, on, off] = CLASSES.map(|c| cell_color(ColorMode::Compare, c, 0.0));
    let material = cell_color(ColorMode::Material, Class::OnPart, 0.0);
    [gouge, faint, strong, on, off, material].map(|c| linear_rgba(c, 1.0))
}

/// One tile of the material on the GPU.
pub(super) struct TileGpu {
    tile: Tile,
    /// Its levels of maxima above level 0, as uploaded.
    pyramid: Pyramid,
    heights: wgpu::Texture,
    codes: wgpu::Texture,
    /// How much sky each cell sees.
    sky: wgpu::Texture,
    grid: wgpu::Buffer,
    pub(super) bind_group: wgpu::BindGroup,
}

/// The material on the GPU, as tiles, sized to one stock.
pub(super) struct StockGpu {
    key: u64,
    size: [u32; 2],
    pub(super) tiles: Vec<TileGpu>,
    /// How the tiles' grids colour the material, as last written; `None` before
    /// they are.
    grids: Option<ColorMode>,
}

/// The material's bind group layout (its grid and its three textures) and the
/// pipeline that traces it.
pub(super) fn stock_pipeline(
    device: &wgpu::Device,
    frame_layout: &wgpu::BindGroupLayout,
) -> (wgpu::RenderPipeline, wgpu::BindGroupLayout) {
    let stock_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("camcraft stock"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Uint,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
        ],
    });
    let stock_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("camcraft stock"),
        source: wgpu::ShaderSource::Wgsl(source(STOCK_SHADER).into()),
    });
    let stock_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("camcraft stock"),
        bind_group_layouts: &[Some(frame_layout), Some(&stock_layout)],
        immediate_size: 0,
    });
    let stock_pipeline = pipeline(
        device,
        &PipelineSpec {
            label: "camcraft stock",
            layout: &stock_pipeline_layout,
            module: &stock_module,
            vertex: "vs",
            buffers: &[],
            blend: None,
            depth_write: true,
            compare: wgpu::CompareFunction::Less,
            bias: wgpu::DepthBiasState::default(),
        },
    );
    (stock_pipeline, stock_layout)
}

impl Gpu {
    /// Upload the material, when it changed. Textures are made once per grid size;
    /// after that, a new image is written whole, and a step of playback writes
    /// only the cells it changed, in the tiles that hold them, and the cells of
    /// their pyramids above.
    ///
    /// The image is shared with playback's thread, which holds it while it works:
    /// it is locked only when something is to be written.
    pub(super) fn upload_stock(&mut self, rs: &RenderState, stock: Option<&StockShow<'_>>) {
        let Some(show) = stock else {
            return;
        };
        let current = self
            .stock
            .as_ref()
            .is_some_and(|s| s.key == show.key && s.grids == Some(show.mode));
        if current && show.changed == StockChange::Nothing {
            return;
        }
        show.image.with(|image| self.upload_image(rs, show, image));
    }

    fn upload_image(&mut self, rs: &RenderState, show: &StockShow<'_>, image: &StockImage) {
        let key = show.key;
        let size = [image.nx, image.ny];
        if size[0] == 0 || size[1] == 0 {
            self.stock = None;
            return;
        }
        let same = self
            .stock
            .as_ref()
            .is_some_and(|s| s.key == key && s.size == size);
        let rects = match &show.changed {
            _ if !same => None,
            StockChange::Nothing => None,
            StockChange::Cells(cells) if cells.iter().all(|r| r.x1 < size[0] && r.y1 < size[1]) => {
                Some(cells.as_slice())
            }
            StockChange::Cells(_) | StockChange::All => None,
        };
        if same && show.changed == StockChange::Nothing {
            // Nothing in the image changed, so the grids stand unless the colours
            // asked for did.
            if self
                .stock
                .as_ref()
                .is_some_and(|s| s.grids != Some(show.mode))
            {
                self.write_grids(rs, image, show.mode);
            }
            return;
        }
        if self.stock.as_ref().is_none_or(|s| s.size != size) {
            let tiles = gpu_tiles::tiles(size[0], size[1])
                .into_iter()
                .map(|t| self.tile_textures(rs, t, image))
                .collect();
            self.stock = Some(StockGpu {
                key,
                size,
                tiles,
                grids: None,
            });
        }
        let Some(gpu) = self.stock.as_mut() else {
            return;
        };
        let floor = narrow(image.floor);
        // First, in parallel, each tile's pyramid brought up to date and what to
        // write of it; then the writes, which the queue takes one at a time.
        let plans: Vec<Option<TilePlan>> = {
            use rayon::prelude::*;
            gpu.tiles
                .par_iter_mut()
                .map(|tile| plan_tile(tile, rects, image, floor))
                .collect()
        };
        for (tile, plan) in gpu.tiles.iter().zip(plans) {
            let Some(plan) = plan else { continue };
            let r = plan.cells;
            match &plan.whole {
                // A step's cells are all on the stock: written straight from the
                // image, which holds them row by row at the stock's width.
                None => {
                    if let Some(from) = tile.tile.stock_rect(r, image.nx, image.ny) {
                        let heights: &[u8] = bytemuck::cast_slice(&image.heights);
                        write_texture(rs, &tile.heights, 0, heights, 4, image.nx, from, r);
                        write_texture(rs, &tile.codes, 0, &image.codes, 1, image.nx, from, r);
                        write_texture(rs, &tile.sky, 0, &image.ao, 1, image.nx, from, r);
                    }
                }
                // The whole tile, apron past the stock's edge and all.
                Some((heights, codes, sky)) => {
                    let whole = CellRect::all(r.width(), r.height());
                    let hs: &[u8] = bytemuck::cast_slice(heights);
                    write_texture(rs, &tile.heights, 0, hs, 4, r.width(), whole, r);
                    write_texture(rs, &tile.codes, 0, codes, 1, r.width(), whole, r);
                    write_texture(rs, &tile.sky, 0, sky, 1, r.width(), whole, r);
                }
            }
            for ((level, r), l) in (1u32..).zip(&plan.levels).zip(&tile.pyramid.levels) {
                write_texture(
                    rs,
                    &tile.heights,
                    level,
                    bytemuck::cast_slice(&l.data),
                    4,
                    l.w,
                    *r,
                    *r,
                );
            }
        }
        gpu.key = key;
        self.write_grids(rs, image, show.mode);
    }

    /// Each tile's grid: where it is, how high its material stands, the colours.
    fn write_grids(&mut self, rs: &RenderState, image: &StockImage, mode: ColorMode) {
        let Some(gpu) = &mut self.stock else {
            return;
        };
        gpu.grids = Some(mode);
        let palette = palette();
        let floor = narrow(image.floor);
        for tile in &gpu.tiles {
            let t = tile.tile;
            #[allow(clippy::cast_precision_loss)]
            let (ox, oy) = (t.x0 as f64, t.y0 as f64);
            let grid = GridUniforms {
                origin_pitch_floor: [
                    narrow(image.origin.x + ox * image.pitch),
                    narrow(image.origin.y + oy * image.pitch),
                    narrow(image.pitch),
                    floor,
                ],
                size: [
                    t.w,
                    t.h,
                    u32::try_from(tile.pyramid.levels.len() + 1).unwrap_or(1),
                    0,
                ],
                top_wall: [
                    tile.pyramid.top().max(floor),
                    WALL_CELLS * narrow(image.pitch),
                    0.0,
                    0.0,
                ],
                core: [
                    t.core.width(),
                    t.core.height(),
                    gpu_tiles::APRON,
                    u32::from(mode == ColorMode::Compare),
                ],
                palette,
                finish: theme::MATERIAL_FINISH.uniform(),
            };
            rs.queue
                .write_buffer(&tile.grid, 0, bytemuck::bytes_of(&grid));
        }
    }

    /// A tile's textures and grid.
    fn tile_textures(&self, rs: &RenderState, tile: Tile, image: &StockImage) -> TileGpu {
        let device = &rs.device;
        let texture = |label, format, [w, h]: [u32; 2], mips: u32| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: mips,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let floor = narrow(image.floor);
        let pyramid = Pyramid::new(tile.tex_w, tile.tex_h, |_, _| floor);
        let mips = u32::try_from(pyramid.levels.len() + 1).unwrap_or(1);
        let heights = texture(
            "camcraft stock heights",
            wgpu::TextureFormat::R32Float,
            [tile.tex_w, tile.tex_h],
            mips,
        );
        let codes = texture(
            "camcraft stock codes",
            wgpu::TextureFormat::R8Uint,
            [tile.w, tile.h],
            1,
        );
        let sky = texture(
            "camcraft stock sky",
            wgpu::TextureFormat::R8Unorm,
            [tile.w, tile.h],
            1,
        );
        let grid = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camcraft stock grid"),
            size: std::mem::size_of::<GridUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let view = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camcraft stock"),
            layout: &self.stock_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: grid.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view(&heights)),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&view(&codes)),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&view(&sky)),
                },
            ],
        });
        TileGpu {
            tile,
            pyramid,
            heights,
            codes,
            sky,
            grid,
            bind_group,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tile's grid, finish and all, is laid out as the stock shader reads it.
    #[test]
    fn a_grid_is_laid_out_as_the_shader_reads_it() {
        let (offset, span) = crate::view::viewport::gpu::tests::layout(STOCK_SHADER, "Grid");
        assert_eq!(
            offset("finish"),
            Some(std::mem::offset_of!(GridUniforms, finish))
        );
        assert_eq!(span, std::mem::size_of::<GridUniforms>());
    }

    /// A rectangle's copy starts at its first cell and strides whole image rows.
    #[test]
    fn a_rect_is_copied_from_its_place_in_the_whole_image() {
        let r = CellRect {
            x0: 3,
            y0: 2,
            x1: 5,
            y1: 4,
        };
        assert_eq!(rect_layout(r, 10, 4), ((2 * 10 + 3) * 4, 40));
        let whole = CellRect {
            x0: 0,
            y0: 0,
            x1: 9,
            y1: 6,
        };
        assert_eq!(rect_layout(whole, 10, 4), (0, 40));
    }
}
