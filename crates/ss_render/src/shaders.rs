//! GLSL 3.30 core shaders. Lighting happens in linear space; every fragment
//! shader writes **sRGB-encoded** color because egui composites in gamma space.

pub const COMMON: &str = r#"
vec3 aces(vec3 x) {
    const float a = 2.51, b = 0.03, c = 2.43, d = 0.59, e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), 0.0, 1.0);
}
vec3 to_srgb(vec3 c) {
    c = clamp(c, 0.0, 1.0);
    return mix(c * 12.92, 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055, step(vec3(0.0031308), c));
}
"#;

pub const SHADED_VS: &str = r#"#version 330 core
layout(location = 0) in vec3 a_pos;
layout(location = 1) in vec3 a_normal;
layout(location = 2) in vec3 a_color;
layout(location = 3) in vec3 a_surface;
uniform mat4 u_view_proj;
uniform mat4 u_model;
out vec3 v_world;
out vec3 v_normal;
out vec3 v_color;
out vec3 v_surface;
void main() {
    vec4 w = u_model * vec4(a_pos, 1.0);
    v_world = w.xyz;
    v_normal = mat3(u_model) * a_normal;
    v_color = a_color;
    v_surface = a_surface;
    gl_Position = u_view_proj * w;
}
"#;

pub const SHADED_FS: &str = r#"#version 330 core
in vec3 v_world;
in vec3 v_normal;
in vec3 v_color;
in vec3 v_surface;
out vec4 f_color;

uniform vec3 u_camera_pos;
uniform vec3 u_sun_dir;
uniform vec3 u_sun_color;
uniform vec3 u_sky_top;
uniform vec3 u_sky_bottom;
uniform float u_ambient;
uniform int u_num_lights;
uniform vec3 u_light_pos[8];
uniform vec3 u_light_color[8];
uniform vec3 u_light_dir[8];
uniform float u_light_range[8];
uniform float u_light_cos[8];
uniform float u_tint;      // brighten (hover / selected)
uniform int u_mode;        // 0 normal, 1 cutter, 2 ghost, 3 x-ray
uniform float u_exposure;

COMMON

const float PI = 3.14159265;

vec3 brdf(vec3 L, vec3 radiance, vec3 N, vec3 V, vec3 diffuse_color, vec3 F0, float rough) {
    float NdotL = max(dot(N, L), 0.0);
    if (NdotL <= 0.0) return vec3(0.0);
    vec3 H = normalize(L + V);
    float NdotV = max(dot(N, V), 1e-3);
    float NdotH = max(dot(N, H), 0.0);
    float a = rough * rough;
    float a2 = a * a;
    float d = NdotH * NdotH * (a2 - 1.0) + 1.0;
    float D = a2 / (PI * d * d);
    float k = (rough + 1.0) * (rough + 1.0) / 8.0;
    float G = (NdotV / (NdotV * (1.0 - k) + k)) * (NdotL / (NdotL * (1.0 - k) + k));
    vec3 F = F0 + (1.0 - F0) * pow(1.0 - max(dot(H, V), 0.0), 5.0);
    vec3 spec = D * G * F / (4.0 * NdotV * NdotL + 1e-4);
    vec3 kd = (1.0 - F);
    return (kd * diffuse_color / PI + spec) * radiance * NdotL;
}

void main() {
    vec3 N = normalize(v_normal);
    if (!gl_FrontFacing) N = -N;
    vec3 V = normalize(u_camera_pos - v_world);
    float rough = clamp(v_surface.x, 0.05, 1.0);
    float metal = clamp(v_surface.y, 0.0, 1.0);
    float glow = max(v_surface.z, 0.0);
    vec3 base = v_color;
    vec3 diffuse_color = base * (1.0 - metal);
    vec3 F0 = mix(vec3(0.04), base, metal);

    // Sky/ground ambient so shaded sides never go black.
    float up = N.y * 0.5 + 0.5;
    vec3 ambient = mix(u_sky_bottom * 0.55, u_sky_top, up) * u_ambient;
    vec3 color = ambient * (diffuse_color + F0 * 0.6);
    // A soft light from the viewer so every face reads.
    color += diffuse_color * 0.18 * max(dot(N, V), 0.0);
    color += brdf(normalize(u_sun_dir), u_sun_color * PI, N, V, diffuse_color, F0, rough);
    for (int i = 0; i < u_num_lights; i++) {
        vec3 Lv = u_light_pos[i] - v_world;
        float dist = length(Lv);
        vec3 L = Lv / max(dist, 1e-4);
        float falloff = clamp(1.0 - pow(dist / max(u_light_range[i], 1e-3), 4.0), 0.0, 1.0);
        float atten = falloff * falloff / (1.0 + dist * dist * 0.05);
        if (u_light_cos[i] > -1.0) {
            float c = dot(-L, normalize(u_light_dir[i]));
            atten *= smoothstep(u_light_cos[i], u_light_cos[i] + 0.05, c);
        }
        color += brdf(L, u_light_color[i] * PI * 2.0 * atten, N, V, diffuse_color, F0, rough);
    }
    color += base * glow;
    color *= 1.0 + u_tint;
    vec3 srgb = to_srgb(aces(color * u_exposure));

    float alpha = 1.0;
    if (u_mode == 1) {
        float stripe = step(0.5, fract((gl_FragCoord.x + gl_FragCoord.y) / 16.0));
        srgb = mix(vec3(0.86, 0.16, 0.12), vec3(1.0, 0.50, 0.45), stripe * 0.6) * (0.75 + 0.25 * abs(dot(N, V)));
        alpha = 0.55;
    } else if (u_mode == 2) {
        srgb = mix(srgb, vec3(0.75, 0.88, 1.0), 0.5);
        alpha = 0.45;
    } else if (u_mode == 3) {
        alpha = 0.4;
    }
    f_color = vec4(srgb, alpha);
}
"#;

pub const FULLSCREEN_VS: &str = r#"#version 330 core
out vec2 v_uv;
void main() {
    vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    v_uv = p;
    gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}
"#;

pub const BACKGROUND_FS: &str = r#"#version 330 core
in vec2 v_uv;
out vec4 f_color;
uniform vec3 u_top;     // sRGB
uniform vec3 u_bottom;  // sRGB
void main() {
    f_color = vec4(mix(u_bottom, u_top, smoothstep(0.0, 1.0, v_uv.y)), 1.0);
}
"#;

pub const GRID_VS: &str = r#"#version 330 core
layout(location = 0) in vec2 a_xz;
uniform mat4 u_view_proj;
uniform vec3 u_center;
uniform float u_extent;
out vec3 v_world;
void main() {
    vec3 w = vec3(u_center.x + a_xz.x * u_extent, 0.0, u_center.z + a_xz.y * u_extent);
    v_world = w;
    gl_Position = u_view_proj * vec4(w, 1.0);
}
"#;

pub const GRID_FS: &str = r#"#version 330 core
in vec3 v_world;
out vec4 f_color;
uniform float u_step;
uniform float u_major;
uniform vec3 u_center;
uniform float u_extent;
uniform vec3 u_camera_pos;

float line_strength(vec2 coord) {
    vec2 w = fwidth(coord);
    vec2 g = abs(fract(coord - 0.5) - 0.5) / max(w, vec2(1e-6));
    float l = min(g.x, g.y);
    // Fade lines that would be denser than a pixel.
    float density = max(w.x, w.y);
    return (1.0 - min(l, 1.0)) * (1.0 - smoothstep(0.3, 0.6, density));
}

void main() {
    vec2 xz = v_world.xz;
    float minor = line_strength(xz / u_step);
    float major = line_strength(xz / (u_step * u_major));
    float dist = length(xz - u_center.xz);
    float fade = 1.0 - smoothstep(u_extent * 0.35, u_extent, dist);
    vec3 view = normalize(v_world - u_camera_pos);
    fade *= smoothstep(0.02, 0.15, abs(view.y));

    vec4 c = vec4(0.18, 0.22, 0.28, 0.0);
    c.a = max(minor * 0.16, major * 0.32);

    // Axis lines: X is red (along z = 0), Z is blue (along x = 0).
    float wz = fwidth(v_world.z) * 1.5;
    float wx = fwidth(v_world.x) * 1.5;
    float ax = 1.0 - smoothstep(0.0, wz, abs(v_world.z));
    float az = 1.0 - smoothstep(0.0, wx, abs(v_world.x));
    if (ax > 0.01) { c = mix(c, vec4(0.86, 0.20, 0.18, 0.9), ax); }
    if (az > 0.01) { c = mix(c, vec4(0.15, 0.40, 0.90, 0.9), az); }
    c.a *= fade;
    if (c.a < 0.003) discard;
    f_color = c;
}
"#;

pub const MASK_VS: &str = r#"#version 330 core
layout(location = 0) in vec3 a_pos;
uniform mat4 u_view_proj;
uniform mat4 u_model;
void main() {
    gl_Position = u_view_proj * u_model * vec4(a_pos, 1.0);
}
"#;

pub const MASK_FS: &str = r#"#version 330 core
out vec4 f_color;
uniform float u_value;
void main() {
    f_color = vec4(u_value, 0.0, 0.0, 1.0);
}
"#;

/// Draws an outline around mask pixels: selection (mask = 1.0) in Roblox blue,
/// hover (mask = 0.5) in a lighter blue.
pub const OUTLINE_FS: &str = r#"#version 330 core
in vec2 v_uv;
out vec4 f_color;
uniform sampler2D u_mask;
uniform vec2 u_texel;
uniform float u_radius;
void main() {
    float center = texture(u_mask, v_uv).r;
    float best = 0.0;
    int r = int(ceil(u_radius));
    for (int y = -r; y <= r; y++) {
        for (int x = -r; x <= r; x++) {
            if (float(x * x + y * y) > u_radius * u_radius + 0.5) continue;
            best = max(best, texture(u_mask, v_uv + vec2(x, y) * u_texel).r);
        }
    }
    if (best <= center + 0.01) discard;
    vec3 sel = vec3(0.0, 0.635, 1.0);     // #00A2FF
    vec3 hov = vec3(0.55, 0.82, 1.0);
    f_color = best > 0.75 ? vec4(sel, 1.0) : vec4(hov, 0.85);
}
"#;

/// Flat-colored overlays (face tints, edges as screen-space quads, points).
pub const OVERLAY_VS: &str = r#"#version 330 core
layout(location = 0) in vec3 a_pos;
layout(location = 1) in vec4 a_color;
uniform mat4 u_view_proj;
uniform mat4 u_model;
out vec4 v_color;
void main() {
    v_color = a_color;
    gl_Position = u_view_proj * u_model * vec4(a_pos, 1.0);
}
"#;

pub const OVERLAY_FS: &str = r#"#version 330 core
in vec4 v_color;
out vec4 f_color;
void main() {
    f_color = v_color;
}
"#;

pub fn with_common(src: &str) -> String {
    src.replace("COMMON", COMMON)
}
