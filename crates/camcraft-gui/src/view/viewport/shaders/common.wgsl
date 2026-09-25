struct Frame {
    view_proj: mat4x4<f32>,
    light: vec4<f32>,
    // Viewport width and height in pixels, line width in pixels, depth pull.
    viewport: vec4<f32>,
    // Visible kinds (roles, then stock and axes), visible operations, segments shown.
    masks: vec4<u32>,
    colors: array<vec4<f32>, 18>,
    // Where the eye is.
    eye: vec4<f32>,
};
@group(0) @binding(0) var<uniform> f: Frame;

// egui shows a native texture as gamma-encoded, so light in linear and encode.
fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}
