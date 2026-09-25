struct LineIn {
    @location(0) a: vec3<f32>,
    @location(1) b: vec3<f32>,
    @location(2) kind: u32,
    @location(3) op: u32,
};
struct LineOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32, l: LineIn) -> LineOut {
    var out: LineOut;
    out.color = vec4<f32>(0.0);
    let kind_on = ((f.masks.x >> l.kind) & 1u) == 1u;
    let is_move = l.kind < 8u;
    let op_on = l.op >= 32u || ((f.masks.y >> l.op) & 1u) == 1u;
    let reached = ii < f.masks.z;
    let ca = f.view_proj * vec4<f32>(l.a, 1.0);
    let cb = f.view_proj * vec4<f32>(l.b, 1.0);
    // Hidden, past the playhead, or crossing the eye: put it outside the clip volume.
    if (!kind_on || (is_move && (!op_on || !reached)) || ca.w <= 1e-6 || cb.w <= 1e-6) {
        out.clip = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        return out;
    }

    let half = f.viewport.xy * 0.5;
    let sa = ca.xy / ca.w * half;
    let sb = cb.xy / cb.w * half;
    var dir = sb - sa;
    if (length(dir) < 1e-6) {
        dir = vec2<f32>(1.0, 0.0);
    }
    dir = normalize(dir);
    let normal = vec2<f32>(-dir.y, dir.x);

    // Two triangles: which end, which side.
    var ends = array<f32, 6>(0.0, 1.0, 1.0, 0.0, 1.0, 0.0);
    var sides = array<f32, 6>(-1.0, -1.0, 1.0, -1.0, 1.0, 1.0);
    let t = ends[vi];
    var clip = mix(ca, cb, t);
    // Picks and the boundary are drawn twice as wide, to read over the part.
    let width = select(f.viewport.z, f.viewport.z * 2.0, l.kind >= 12u && l.kind <= 14u);
    let offset = normal * sides[vi] * width * 0.5 + dir * (t * 2.0 - 1.0) * width * 0.5;
    clip = vec4<f32>(clip.xy + offset / half * clip.w, clip.z, clip.w);
    // A cut runs exactly on the surface it follows; pull it towards the eye so the
    // surface cannot flicker through it. Scaled by w, so the pull is constant in NDC.
    clip.z = clip.z - f.viewport.w * clip.w;
    out.clip = clip;
    out.color = f.colors[l.kind];
    return out;
}

@fragment
fn fs(i: LineOut) -> @location(0) vec4<f32> {
    return vec4<f32>(to_srgb(i.color.rgb), 1.0);
}
