// LFILTER's color conversion, the same as filter::shift_color does on the
// CPU: the usual HSV formulas on the premultiplied texel, with the coverage
// kept.
//
// SDL's bindings for a fragment shader: textures in set 2, uniforms in set 3.
// The vertex shader is SDL_Renderer's own. See shaders/README.md for how this
// becomes the .spv beside it.
#version 450

layout(location = 0) in vec4 v_color;
layout(location = 1) in vec2 v_uv;
layout(location = 0) out vec4 o_color;

layout(set = 2, binding = 0) uniform sampler2D u_texture;
layout(set = 3, binding = 0) uniform Shift {
    // Hue, saturation and value, each as a fraction of its full range. w is 1
    // when a clear pixel is left unchanged (compose mode 1).
    vec4 u_shift;
};

void main() {
    const float EPSILON = 1e-10;
    vec4 texel = texelFetch(u_texture, ivec2(gl_FragCoord.xy), 0);
    if (u_shift.w != 0.0 && texel == vec4(0.0)) {
        o_color = vec4(0.0);
        return;
    }
    vec4 p = texel.g < texel.b
        ? vec4(texel.b, texel.g, -1.0, 2.0 / 3.0)
        : vec4(texel.g, texel.b, 0.0, -1.0 / 3.0);
    vec4 q = texel.r < p.x
        ? vec4(p.x, p.y, p.w, texel.r)
        : vec4(texel.r, p.y, p.z, p.x);
    float d = q.x - min(q.w, q.y);
    float h = fract(abs(q.z + (q.w - q.y) / (6.0 * d + EPSILON)) + u_shift.x);
    float s = d / (q.x + EPSILON);
    s = s == 0.0 ? 0.0 : clamp(s + u_shift.y, 0.0, 1.0);
    float v = clamp(q.x + u_shift.z, 0.0, 1.0);
    vec3 t = clamp(abs(fract(h + vec3(1.0, 2.0 / 3.0, 1.0 / 3.0)) * 6.0 - 3.0) - 1.0, 0.0, 1.0);
    vec3 c = v * (1.0 + s * (t - 1.0));
    // Rounds to the nearest, with halves rounded down, like the CPU.
    o_color = vec4(floor(c * 255.0 + 0.499) / 255.0, texel.a);
}
