// Physically-based shading for the glTF viewer: metallic-roughness Cook-Torrance
// lit by an image-based skybox plus three fixed studio lights, so the model is
// lit the same way whichever backend (WebGPU or WebGL2) ends up running it.
//
// The skybox is two images (see `environment.rs`): a blurred equirectangular
// panorama of a room, drawn as the backdrop and reflected as the soft half of
// the surroundings, and a photograph hung in front of the model like a studio
// light card, which is what the polished metal has to catch and show.

struct Globals {
    view_projection: mat4x4<f32>,
    // Turns a clip-space position back into a world-space ray, which is how the
    // skybox pass works out what each pixel is looking at.
    inverse_view_projection: mat4x4<f32>,
    camera_position: vec4<f32>,
    // x: 1.0 when the surface format is not sRGB and the shader has to encode
    // gamma itself. y: 1.0 when the skybox images are bound, 0.0 when the
    // viewer fell back to the procedural gradient. z, w: the deepest mip level
    // of the background and foreground images.
    flags: vec4<f32>,
};

struct MaterialUniform {
    base_color: vec4<f32>,
    // x: metallic, y: roughness, z: 1.0 when the base colour texture is real,
    // w: opacity.
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var<uniform> material: MaterialUniform;
@group(2) @binding(0) var base_color_texture: texture_2d<f32>;
@group(2) @binding(1) var base_color_sampler: sampler;
@group(3) @binding(0) var background_texture: texture_2d<f32>;
@group(3) @binding(1) var foreground_texture: texture_2d<f32>;
@group(3) @binding(2) var environment_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    // Node transforms are baked into the vertices on the CPU, so world space and
    // model space are the same thing here.
    output.clip_position = globals.view_projection * vec4<f32>(input.position, 1.0);
    output.world_position = input.position;
    output.normal = input.normal;
    output.uv = input.uv;
    return output;
}

const PI: f32 = 3.14159265359;

// Three-point studio rig: direction, colour and intensity per light. The skybox
// supplies the soft, wrapping light; these stay for the crisp highlights and the
// shaping a photograph on its own cannot give.
const LIGHT_DIRECTIONS = array<vec3<f32>, 3>(
    vec3<f32>(0.55, 0.72, 0.42),   // key, from the upper right
    vec3<f32>(-0.68, 0.18, 0.52),  // fill, cooler and softer
    vec3<f32>(-0.1, 0.35, -0.85),  // rim, from behind
);
const LIGHT_COLORS = array<vec3<f32>, 3>(
    vec3<f32>(1.0, 0.98, 0.94) * 2.5,
    vec3<f32>(0.72, 0.82, 1.0) * 0.9,
    vec3<f32>(1.0, 0.95, 0.9) * 1.5,
);

// Where the foreground image hangs, as a direction from the model, and how much
// of the sphere it covers: half its width and height on a plane one unit away.
// In front and a little above, which is where a room's light comes from.
const CARD_DIRECTION = vec3<f32>(0.18, 0.30, 1.0);
const CARD_HALF_SIZE = vec2<f32>(1.05, 1.05);
// The panorama is a photograph, not a light probe, so its values top out at
// white. These push it back up to something that reads as light.
const BACKGROUND_EXPOSURE: f32 = 1.55;
const CARD_EXPOSURE: f32 = 2.6;
/// How hard the brightest parts of the skybox are pushed past white, so windows
/// and lit surfaces survive tone mapping as actual highlights.
const HIGHLIGHT_GAIN: f32 = 2.2;
/// Exposure and vignette for the backdrop, which has to stay behind the subject.
const BACKDROP_EXPOSURE: f32 = 0.62;
const BACKDROP_VIGNETTE: f32 = 0.55;
/// Mip level the backdrop is sampled at: the panorama is already soft, and a
/// touch more keeps it from competing with the model.
const BACKDROP_LOD: f32 = 1.0;

fn distribution_ggx(n_dot_h: f32, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let denominator = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    return a2 / max(PI * denominator * denominator, 1e-5);
}

fn geometry_smith(n_dot_v: f32, n_dot_l: f32, roughness: f32) -> f32 {
    let k = (roughness + 1.0) * (roughness + 1.0) / 8.0;
    let ggx_v = n_dot_v / (n_dot_v * (1.0 - k) + k);
    let ggx_l = n_dot_l / (n_dot_l * (1.0 - k) + k);
    return ggx_v * ggx_l;
}

fn fresnel_schlick(cosine: f32, f0: vec3<f32>) -> vec3<f32> {
    return f0 + (vec3<f32>(1.0) - f0) * pow(clamp(1.0 - cosine, 0.0, 1.0), 5.0);
}

// Narkowicz's ACES fit: keeps the bright speculars on the metal frame from
// clipping to flat white.
fn tonemap(color: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((color * (a * color + b)) / (color * (c * color + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

/// Fakes the dynamic range a photograph does not have: the brighter a texel is,
/// the further past 1.0 it is pushed, so a window stays a highlight in a
/// reflection instead of flattening into grey.
fn expand_range(color: vec3<f32>) -> vec3<f32> {
    let luminance = dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
    return color * (1.0 + HIGHLIGHT_GAIN * luminance * luminance);
}

/// Which mip level a surface of this roughness reflects with. The square root
/// spreads the levels out: most of the visible change happens while a surface is
/// still fairly polished.
fn environment_lod(roughness: f32, max_lod: f32) -> f32 {
    return clamp(sqrt(roughness) * max_lod, 0.0, max_lod);
}

/// Sample the panorama along `direction`, equirectangular: longitude across,
/// latitude down.
fn background_sample(direction: vec3<f32>, lod: f32) -> vec3<f32> {
    let d = normalize(direction);
    let u = atan2(d.x, -d.z) / (2.0 * PI) + 0.5;
    let v = acos(clamp(d.y, -1.0, 1.0)) / PI;
    return textureSampleLevel(background_texture, environment_sampler, vec2<f32>(u, v), lod).rgb;
}

/// Sample the light card along `direction`.
///
/// Returns the colour in `rgb` and how much of the card is in the way in `a`,
/// which falls off towards its edges so the rectangle does not cut a hard line
/// across a reflection.
fn card_sample(direction: vec3<f32>, lod: f32) -> vec4<f32> {
    let forward = normalize(CARD_DIRECTION);
    let right = normalize(cross(vec3<f32>(0.0, 1.0, 0.0), forward));
    let up = cross(forward, right);

    let d = normalize(direction);
    let facing = dot(d, forward);
    if (facing <= 0.1) {
        return vec4<f32>(0.0);
    }

    // Where the ray crosses the plane the card sits on, in the card's own axes.
    let on_plane = d / facing;
    let offset = vec2<f32>(dot(on_plane, right), dot(on_plane, up)) / CARD_HALF_SIZE;
    if (abs(offset.x) >= 1.0 || abs(offset.y) >= 1.0) {
        return vec4<f32>(0.0);
    }

    let edge = (1.0 - smoothstep(0.72, 1.0, abs(offset.x)))
        * (1.0 - smoothstep(0.72, 1.0, abs(offset.y)));
    let uv = vec2<f32>(offset.x * 0.5 + 0.5, 0.5 - offset.y * 0.5);
    let color = textureSampleLevel(foreground_texture, environment_sampler, uv, lod).rgb;

    return vec4<f32>(color, edge);
}

// The gradient the viewer falls back to when the skybox images could not be
// loaded: a sky-to-ground ramp with a soft highlight for each of the three
// lights. Without something there, polished metal has nothing to reflect and
// renders almost black.
fn procedural_environment(direction: vec3<f32>, roughness: f32) -> vec3<f32> {
    let horizon = smoothstep(-0.35, 0.35, direction.y);
    var color = mix(vec3<f32>(0.05, 0.05, 0.06), vec3<f32>(0.5, 0.56, 0.68), horizon);

    // Smooth surfaces reflect the lights as small bright spots, rough ones as
    // broad washes.
    let sharpness = mix(5.0, 220.0, 1.0 - roughness);
    var directions = LIGHT_DIRECTIONS;
    var colors = LIGHT_COLORS;
    for (var i = 0; i < 3; i = i + 1) {
        let alignment = max(dot(direction, normalize(directions[i])), 0.0);
        color += colors[i] * 0.22 * pow(alignment, sharpness);
    }

    return color;
}

/// What the surroundings send back along `direction`, blurred for `roughness`:
/// the panorama, with the light card composited over the part of the sphere it
/// covers.
fn environment(direction: vec3<f32>, roughness: f32) -> vec3<f32> {
    if (globals.flags.y < 0.5) {
        return procedural_environment(direction, roughness);
    }

    var color = background_sample(direction, environment_lod(roughness, globals.flags.z))
        * BACKGROUND_EXPOSURE;

    // The card is an object in the room, so it hides the panorama behind it
    // rather than adding to it.
    let card = card_sample(direction, environment_lod(roughness, globals.flags.w));
    color = mix(color, card.rgb * CARD_EXPOSURE, card.a);

    return expand_range(color);
}

struct SkyOutput {
    @builtin(position) clip_position: vec4<f32>,
    // Normalised device coordinates, so the fragment shader can unproject the
    // exact pixel rather than interpolate a direction that a perspective divide
    // has already bent.
    @location(0) ndc: vec2<f32>,
};

/// One triangle covering the whole viewport, built from the vertex index alone -
/// the skybox needs no vertex buffer.
@vertex
fn vs_sky(@builtin(vertex_index) index: u32) -> SkyOutput {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    let ndc = corner * 2.0 - vec2<f32>(1.0);

    var output: SkyOutput;
    // At the far plane, so the model - drawn afterwards - is always in front.
    output.clip_position = vec4<f32>(ndc, 1.0, 1.0);
    output.ndc = ndc;
    return output;
}

@fragment
fn fs_sky(input: SkyOutput) -> @location(0) vec4<f32> {
    let near = globals.inverse_view_projection * vec4<f32>(input.ndc, 0.0, 1.0);
    let far = globals.inverse_view_projection * vec4<f32>(input.ndc, 1.0, 1.0);
    let direction = normalize(far.xyz / far.w - near.xyz / near.w);

    // Straight off the panorama, not through the tone mapper: the backdrop is a
    // photograph and should look like one, only dimmer than the subject.
    var color = background_sample(direction, BACKDROP_LOD) * BACKDROP_EXPOSURE;

    // Darken towards the corners so the model keeps the eye in the middle.
    let vignette = 1.0 - BACKDROP_VIGNETTE * dot(input.ndc, input.ndc) * 0.5;
    color = color * vignette;

    if (globals.flags.x > 0.5) {
        color = pow(color, vec3<f32>(1.0 / 2.2));
    }

    return vec4<f32>(color, 1.0);
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    var albedo = material.base_color.rgb;
    if (material.params.z > 0.5) {
        albedo = albedo * textureSample(base_color_texture, base_color_sampler, input.uv).rgb;
    }

    let metallic = material.params.x;
    let roughness = clamp(material.params.y, 0.045, 1.0);

    // Back faces of the thin glass shells would otherwise be lit from behind.
    var normal = normalize(input.normal);
    let view_direction = normalize(globals.camera_position.xyz - input.world_position);
    if (dot(normal, view_direction) < 0.0) {
        normal = -normal;
    }

    let n_dot_v = max(dot(normal, view_direction), 1e-4);
    let f0 = mix(vec3<f32>(0.04), albedo, metallic);

    var radiance = vec3<f32>(0.0);
    // Copied into function scope so the loop can index them dynamically.
    var directions = LIGHT_DIRECTIONS;
    var colors = LIGHT_COLORS;
    for (var i = 0; i < 3; i = i + 1) {
        let light_direction = normalize(directions[i]);
        let half_vector = normalize(light_direction + view_direction);
        let n_dot_l = max(dot(normal, light_direction), 0.0);
        if (n_dot_l <= 0.0) {
            continue;
        }

        let n_dot_h = max(dot(normal, half_vector), 0.0);
        let distribution = distribution_ggx(n_dot_h, roughness);
        let geometry = geometry_smith(n_dot_v, n_dot_l, roughness);
        let fresnel = fresnel_schlick(max(dot(half_vector, view_direction), 0.0), f0);

        let specular = distribution * geometry * fresnel / max(4.0 * n_dot_v * n_dot_l, 1e-4);
        let diffuse = (vec3<f32>(1.0) - fresnel) * (1.0 - metallic) * albedo / PI;

        radiance += (diffuse + specular) * colors[i] * n_dot_l;
    }

    // Ambient diffuse: the light arriving from the whole skybox, which the
    // deepest mip levels already average for us. Metals have no diffuse
    // response, so it fades out with metallic.
    let irradiance = environment(normal, 1.0);
    let ambient = irradiance * albedo * (1.0 - metallic) * 0.85;

    // Ambient specular: what the surface reflects of the surroundings. The
    // roughness-aware Fresnel keeps rough metal from turning into a mirror.
    let reflection = reflect(-view_direction, normal);
    let fresnel_ambient = f0 + (max(vec3<f32>(1.0 - roughness), f0) - f0)
        * pow(clamp(1.0 - n_dot_v, 0.0, 1.0), 5.0);
    let ambient_specular = environment(reflection, roughness) * fresnel_ambient;

    var color = tonemap(radiance + ambient + ambient_specular);
    if (globals.flags.x > 0.5) {
        color = pow(color, vec3<f32>(1.0 / 2.2));
    }

    // Glass is most visible where it turns away from the viewer, so fade the
    // opacity back in towards the silhouette instead of using a flat alpha.
    let rim = pow(1.0 - n_dot_v, 3.0);
    let alpha = clamp(mix(material.params.w, 1.0, rim), 0.0, 1.0);

    return vec4<f32>(color, alpha);
}
