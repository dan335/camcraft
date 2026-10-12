struct Frame {
    view_proj: mat4x4<f32>,
    // Viewport width and height in pixels, line width in pixels, depth pull.
    viewport: vec4<f32>,
    // Visible kinds of line (a bit each), visible operations (a bit each), lines
    // shown, and one more than the one operation shown alone (zero for none).
    masks: vec4<u32>,
    // Where the eye is.
    eye: vec4<f32>,
    // Looking orthographically, which way the view looks, and how far back from
    // what it shows a line of sight starts; all zero in perspective.
    sight: vec4<f32>,
    // Written once, after what changes each frame: a colour per kind of line,
    // the moves' roles, then the rest.
    colors: array<vec4<f32>, LINE_KINDS>,
};
@group(0) @binding(0) var<uniform> f: Frame;

// The light rig, dcpwood's viewers' (`addStandardLights`), turned to Z up: a
// hemisphere from a warm white sky to a warm grey ground; a warm key from high
// above the front left of where the camera starts, so tops read lighter than
// walls; a faint cool fill from its right and a cool rim from behind; and the
// room's even light. Part and material are lit by the same rig, so they read as
// one scene. Directions are the way to each light, in the world.
const KEY_DIR: vec3<f32> = vec3<f32>(0.04, -0.537, 0.843);
const KEY: vec3<f32> = vec3<f32>(0.62, 0.572, 0.513);
const FILL_DIR: vec3<f32> = vec3<f32>(0.933, -0.012, 0.359);
const FILL: vec3<f32> = vec3<f32>(0.117, 0.121, 0.123);
const RIM_DIR: vec3<f32> = vec3<f32>(-0.384, 0.812, 0.439);
const RIM: vec3<f32> = vec3<f32>(0.098, 0.11, 0.112);
const SKY: vec3<f32> = vec3<f32>(0.213, 0.212, 0.207);
const GROUND: vec3<f32> = vec3<f32>(0.102, 0.095, 0.083);
// The room's light, the same from every side.
const ROOM: f32 = 0.2;

const PI: f32 = 3.14159265;

// What the hemisphere and the room send along `d`.
fn around(d: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(ROOM) + mix(GROUND, SKY, 0.5 * d.z + 0.5);
}

// GGX's distribution of microfacet normals, for `a` roughness squared.
fn ggx(a: f32, n_h: f32) -> f32 {
    let a2 = a * a;
    let d = n_h * n_h * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d);
}

// Smith's height-correlated visibility for GGX, as three.js's.
fn visibility(a: f32, n_l: f32, n_v: f32) -> f32 {
    let a2 = a * a;
    let gv = n_l * sqrt(a2 + (1.0 - a2) * n_v * n_v);
    let gl = n_v * sqrt(a2 + (1.0 - a2) * n_l * n_l);
    return 0.5 / max(gv + gl, 1e-6);
}

// Karis's analytic fit to the split-sum table: how much of what surrounds it a
// surface of reflectance `f0` and `roughness` reflects, seen at `n_v`.
fn env_brdf(f0: vec3<f32>, roughness: f32, n_v: f32) -> vec3<f32> {
    let r = roughness * vec4<f32>(-1.0, -0.0275, -0.572, 0.022)
        + vec4<f32>(1.0, 0.0425, 1.04, -0.04);
    let a004 = min(r.x * r.x, exp2(-9.28 * n_v)) * r.x + r.y;
    let ab = vec2<f32>(-1.04, 1.04) * a004 + r.zw;
    return f0 * ab.x + ab.y;
}

// `albedo` (linear) of `finish` (roughness, metalness) lit by the rig, facing
// `normal` and seen from `to_eye`, tone mapped: three.js's standard material,
// its lights' intensities taken as they were tuned here (three.js's are π less).
// Two-sided, so a surface seen from behind is lit as its front.
fn lit(albedo: vec3<f32>, finish: vec4<f32>, normal: vec3<f32>, to_eye: vec3<f32>) -> vec3<f32> {
    let v = normalize(to_eye);
    var n = normalize(normal);
    if (dot(n, v) < 0.0) {
        n = -n;
    }
    let roughness = clamp(finish.x, 0.04, 1.0);
    let metal = clamp(finish.y, 0.0, 1.0);
    let a = roughness * roughness;
    let diffuse = albedo * (1.0 - metal);
    let f0 = mix(vec3<f32>(0.04), albedo, metal);
    let n_v = max(dot(n, v), 1e-4);

    var out = diffuse * around(n);
    let dirs = array<vec3<f32>, 3>(KEY_DIR, FILL_DIR, RIM_DIR);
    let colors = array<vec3<f32>, 3>(KEY, FILL, RIM);
    for (var i = 0; i < 3; i++) {
        let l = dirs[i];
        let n_l = max(dot(n, l), 0.0);
        let h = normalize(l + v);
        let fresnel = f0 + (1.0 - f0) * pow(1.0 - max(dot(v, h), 0.0), 5.0);
        let specular = PI * fresnel * visibility(a, n_l, n_v) * ggx(a, max(dot(n, h), 0.0));
        out += colors[i] * n_l * (diffuse + specular);
    }
    // What the room reflects, from the way a mirror would see it.
    out += env_brdf(f0, roughness, n_v) * around(reflect(-v, n));
    return neutral(out);
}

// Khronos PBR Neutral tone mapping, as three.js's `NeutralToneMapping`: colours
// below its knee pass unchanged, brighter ones roll off without shifting hue.
fn neutral(c_in: vec3<f32>) -> vec3<f32> {
    let start = 0.8 - 0.04;
    let desaturation = 0.15;
    let x = min(c_in.r, min(c_in.g, c_in.b));
    let offset = select(0.04, x - 6.25 * x * x, x < 0.08);
    let c = c_in - offset;
    let peak = max(c.r, max(c.g, c.b));
    if (peak < start) {
        return c;
    }
    let d = 1.0 - start;
    let new_peak = 1.0 - d * d / (peak + d - start);
    let scaled = c * (new_peak / peak);
    let g = 1.0 - 1.0 / (desaturation * (peak - new_peak) + 1.0);
    return mix(scaled, vec3<f32>(new_peak), g);
}

// egui shows a native texture as gamma-encoded, so light in linear and encode.
fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}
