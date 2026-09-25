struct Object {
    model: mat4x4<f32>,
    color: vec4<f32>,
};
@group(1) @binding(0) var<uniform> o: Object;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
};
struct VertexOut {
    // Invariant, so a picked face drawn again over the part lands at exactly the
    // part's depth, and the equal depth test passes.
    @invariant @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
};

@vertex
fn vs(v: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = f.view_proj * o.model * vec4<f32>(v.position, 1.0);
    // Objects are only ever turned and moved, never scaled, so the model matrix
    // turns normals too.
    out.normal = (o.model * vec4<f32>(v.normal, 0.0)).xyz;
    return out;
}

@fragment
fn fs(i: VertexOut) -> @location(0) vec4<f32> {
    // Two-sided: a mesh from an STL may face either way, and both sides are part.
    let n = normalize(i.normal);
    let diffuse = abs(dot(n, f.light.xyz));
    let lit = o.color.rgb * (0.25 + 0.75 * diffuse);
    return vec4<f32>(to_srgb(lit), o.color.a);
}
