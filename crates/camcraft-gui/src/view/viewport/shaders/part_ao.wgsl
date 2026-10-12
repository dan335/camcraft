struct Ao {
    inv_proj: mat4x4<f32>,
    // The view's size in pixels, then half of it.
    size: vec4<f32>,
    // How far to look, in millimetres; pixels per millimetre at a distance of
    // one, or looking orthographically, anywhere; how strongly to darken; 1 when
    // the view is orthographic.
    params: vec4<f32>,
};
@group(0) @binding(0) var<uniform> a: Ao;
@group(0) @binding(1) var depth: texture_depth_multisampled_2d;
@group(0) @binding(2) var found: texture_2d<f32>;

const PI: f32 = 3.14159265;
const HALF_PI: f32 = 1.57079633;
const SLICES: i32 = 3;
const STEPS: i32 = 6;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    // One triangle over the whole view.
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

fn depth_at(px: vec2<i32>) -> f32 {
    let c = clamp(px, vec2<i32>(0), vec2<i32>(a.size.xy) - 1);
    return textureLoad(depth, c, 0);
}

// Pixel `px`'s point, in the eye's frame: the eye looks down -z.
fn view_at(px: vec2<i32>) -> vec3<f32> {
    let d = depth_at(px);
    let uv = (vec2<f32>(px) + 0.5) / a.size.xy;
    let v = a.inv_proj * vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, d, 1.0);
    return v.xyz / v.w;
}

// A 4 by 4 pattern of sixteen steps, 0..1: each pixel of a block turns its
// directions a different way, and the blur over the block evens them out.
fn pattern(p: vec2<i32>) -> f32 {
    let m = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0,
                           3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    let i = (p.y & 3) * 4 + (p.x & 3);
    return (m[i] + 0.5) / 16.0;
}

@fragment
fn gtao(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let h = vec2<i32>(frag.xy);
    let px = h * 2;
    if (depth_at(px) >= 1.0) { return vec4<f32>(1.0); }
    let p = view_at(px);
    // The surface's facing, from the nearer of each pair of neighbours, so an
    // edge does not bend it.
    let l = view_at(px - vec2<i32>(2, 0));
    let r = view_at(px + vec2<i32>(2, 0));
    let u = view_at(px - vec2<i32>(0, 2));
    let d = view_at(px + vec2<i32>(0, 2));
    let dx = select(p - l, r - p, abs(r.z - p.z) < abs(p.z - l.z));
    let dy = select(p - u, d - p, abs(d.z - p.z) < abs(p.z - u.z));
    // Towards the eye: along the view everywhere when it is orthographic.
    let ortho = a.params.w > 0.5;
    let v = select(normalize(-p), vec3<f32>(0.0, 0.0, 1.0), ortho);
    var n = normalize(cross(dx, dy));
    if (dot(n, v) < 0.0) { n = -n; }

    let reach = a.params.x;
    let reach_px = select(reach * a.params.y / max(-p.z, 1e-4), reach * a.params.y, ortho);
    if (reach_px < 1.0) { return vec4<f32>(1.0); }
    // What is found counts less over the last share of the reach.
    let fade = 0.615 * reach;
    let fade_mul = -1.0 / fade;
    let fade_add = (reach - fade) / fade + 1.0;
    let nearest = 1.3 / reach_px;
    let turn = pattern(h);
    let shift = fract(turn * 7.0 + 0.37);

    var open = 0.0;
    for (var s = 0; s < SLICES; s++) {
        let phi = (f32(s) + turn) / f32(SLICES) * PI;
        // Along the screen (y down), and the same way in the eye's frame (y up).
        let omega = vec2<f32>(cos(phi), -sin(phi));
        let dir = vec3<f32>(cos(phi), sin(phi), 0.0);
        let ortho = dir - dot(dir, v) * v;
        let axis = normalize(cross(dir, v));
        let proj_n = n - axis * dot(n, axis);
        let len = length(proj_n);
        let cos_n = clamp(dot(proj_n, v) / max(len, 1e-6), 0.0, 1.0);
        let an = sign(dot(ortho, proj_n)) * acos(cos_n);
        let low0 = cos(an + HALF_PI);
        let low1 = cos(an - HALF_PI);
        var hc0 = low0;
        var hc1 = low1;
        for (var j = 0; j < STEPS; j++) {
            var t = (f32(j) + shift) / f32(STEPS);
            t = t * t + nearest;
            let o = vec2<i32>(round(omega * t * reach_px));
            let d0 = view_at(px + o) - p;
            let d1 = view_at(px - o) - p;
            let l0 = length(d0);
            let l1 = length(d1);
            let w0 = clamp(l0 * fade_mul + fade_add, 0.0, 1.0);
            let w1 = clamp(l1 * fade_mul + fade_add, 0.0, 1.0);
            hc0 = max(hc0, mix(low0, dot(d0 / max(l0, 1e-6), v), w0));
            hc1 = max(hc1, mix(low1, dot(d1 / max(l1, 1e-6), v), w1));
        }
        let h0 = an + clamp(-acos(clamp(hc1, -1.0, 1.0)) - an, -HALF_PI, HALF_PI);
        let h1 = an + clamp(acos(clamp(hc0, -1.0, 1.0)) - an, -HALF_PI, HALF_PI);
        let arc0 = (cos_n + 2.0 * h0 * sin(an) - cos(2.0 * h0 - an)) / 4.0;
        let arc1 = (cos_n + 2.0 * h1 * sin(an) - cos(2.0 * h1 - an)) / 4.0;
        open += len * (arc0 + arc1);
    }
    return vec4<f32>(clamp(open / f32(SLICES), 0.0, 1.0), 0.0, 0.0, 1.0);
}

// The 4 by 4 block round each pixel, leaving out any not on its surface.
@fragment
fn blur(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let h = vec2<i32>(frag.xy);
    if (depth_at(h * 2) >= 1.0) { return vec4<f32>(1.0); }
    let z = view_at(h * 2).z;
    let most = vec2<i32>(a.size.zw) - 1;
    var sum = 0.0;
    var weight = 0.0;
    for (var dy = -1; dy <= 2; dy++) {
        for (var dx = -1; dx <= 2; dx++) {
            let q = clamp(h + vec2<i32>(dx, dy), vec2<i32>(0), most);
            if (depth_at(q * 2) >= 1.0) { continue; }
            // Off the surface: further in depth than a share of the depth, or
            // looking orthographically, where depth is not distance, of the reach.
            let off = select(0.03 * abs(z), 0.25 * a.params.x, a.params.w > 0.5);
            if (abs(view_at(q * 2).z - z) > off) { continue; }
            sum += textureLoad(found, q, 0).r;
            weight += 1.0;
        }
    }
    return vec4<f32>(sum / max(weight, 1.0), 0.0, 0.0, 1.0);
}

// Each pixel of the part darkened by its occlusion; the rest left as it is.
@fragment
fn composite(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let px = vec2<i32>(frag.xy);
    if (depth_at(px) >= 1.0) { return vec4<f32>(1.0); }
    let h = clamp(px / 2, vec2<i32>(0), vec2<i32>(a.size.zw) - 1);
    let k = mix(1.0, textureLoad(found, h, 0).r, a.params.z);
    return vec4<f32>(k, k, k, 1.0);
}
