// Shading for the kitchen constructor: metallic-roughness Cook-Torrance lit by
// the three pendant lamps, a soft room ambient and reflections of the skybox, so
// the polished steel and aluminium fronts have something to mirror.
//
// The environment sampling and the BRDF follow `crates/wasm-viewer/src/shader.wgsl`;
// what is new here is point lights, per-placement transforms, the contact shadow
// under the run of modules and the LED glow a hovered slot lights up with.

struct Globals {
    view_projection: mat4x4<f32>,
    camera_position: vec4<f32>,
    // x: 1.0 when the surface format is not sRGB and the shader has to encode
    // gamma itself. y: 1.0 when the skybox images are bound, 0.0 when the room
    // reflects a procedural gradient instead. z, w: the deepest mip level of the
    // background and foreground images.
    flags: vec4<f32>,
    // xyz: where each lamp's light is. w: how far it reaches, in metres.
    light_positions: array<vec4<f32>, 3>,
    // rgb: colour times intensity.
    light_colors: array<vec4<f32>, 3>,
    // The space the run of modules takes up against the wall: x and z of its
    // min and max corners, and its height in `run_max.y`. The floor and the wall
    // darken around it, which is what stands the modules on the floor.
    // `run_min.w` is half the wall's width.
    run_min: vec4<f32>,
    run_max: vec4<f32>,
    // The box of the most lit-up slot, with how lit up it is in `hover_min.w`.
    hover_min: vec4<f32>,
    hover_max: vec4<f32>,
};

struct Placement {
    model: mat4x4<f32>,
    // x: how lit up the slot is, 0 to 1. y: 1.0 when the slot holds the
    // placeholder, which glows like LED-lit glass rather than tinting a module.
    highlight: vec4<f32>,
    // The slot's box in world space, whose edges light up like LED strips.
    // Zero-sized for the room, which never glows.
    bounds_min: vec4<f32>,
    bounds_max: vec4<f32>,
};

struct MaterialUniform {
    base_color: vec4<f32>,
    // x: metallic, y: roughness, z: 1.0 when the base colour texture is real,
    // w: opacity.
    params: vec4<f32>,
    // rgb: emitted light. w: which surface this is - 0 anything, 1 the floor,
    // 2 the wall - for the contact shadow.
    emissive: vec4<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var<uniform> placement: Placement;
@group(2) @binding(0) var<uniform> material: MaterialUniform;
@group(2) @binding(1) var base_color_texture: texture_2d<f32>;
@group(2) @binding(2) var base_color_sampler: sampler;
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
    let world = placement.model * vec4<f32>(input.position, 1.0);

    var output: VertexOutput;
    output.clip_position = globals.view_projection * world;
    output.world_position = world.xyz;
    output.normal = (placement.model * vec4<f32>(input.normal, 0.0)).xyz;
    output.uv = input.uv;
    return output;
}

const PI: f32 = 3.14159265359;

const SURFACE_FLOOR: f32 = 1.0;
const SURFACE_WALL: f32 = 2.0;

// The light a room with white walls bounces around: off the ceiling and walls
// above, a little darker and warmer off the floor below.
const SKY_AMBIENT = vec3<f32>(0.52, 0.51, 0.49);
const GROUND_AMBIENT = vec3<f32>(0.3, 0.28, 0.25);
// What a mirror-like surface sees of the room: a white ceiling above, the
// beige wall around the horizon and the tiled floor below.
const ROOM_CEILING = vec3<f32>(1.0, 0.99, 0.97);
const ROOM_WALL = vec3<f32>(0.8, 0.77, 0.71);
const ROOM_FLOOR = vec3<f32>(0.62, 0.6, 0.56);
// How much of a reflection is the skybox photograph rather than the plain room:
// a little on rough or painted surfaces, nearly all of it on polished metal,
// which should mirror the room's window, dark walls and furniture as sharply
// and with as much contrast as a mirror would.
const PHOTO_REFLECTION: f32 = 0.35;
const POLISHED_PHOTO_REFLECTION: f32 = 0.95;
const PHOTO_SATURATION: f32 = 0.6;
// Reflections for metals and for everything else: a painted wall should only
// have a sheen, not a mirror image.
const METAL_REFLECTION: f32 = 1.0;
const DIELECTRIC_REFLECTION: f32 = 0.3;

// The glow of a lit-up slot: a cool, slightly blue LED white that stands out
// against the warm wall and floor.
const LED_COLOR = vec3<f32>(0.22, 0.66, 1.0);
// Half the width of an LED edge strip, in metres.
const LED_EDGE_WIDTH: f32 = 0.012;
// An empty slot's edges glow faintly even when nothing is over it, so the
// placeholders read as places to click rather than as panes of glass.
const PLACEHOLDER_IDLE_EDGE: f32 = 0.35;

// The room photograph, hung in front of the run and tilted 35 degrees down, which
// is where upright fronts seen from standing height mirror. It spans 90 degrees
// across at the photo's 3:2 aspect. The background panorama projects the same
// photo with the same card, so keep both in step with
// `apps/web/public/env/living-room-*.png`.
const CARD_DIRECTION = vec3<f32>(0.0, -0.5736, 0.8192);
const CARD_HALF_SIZE = vec2<f32>(1.0, 0.6667);
const BACKGROUND_EXPOSURE: f32 = 1.3;
const CARD_EXPOSURE: f32 = 2.2;
const HIGHLIGHT_GAIN: f32 = 2.2;

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

// Narkowicz's ACES fit.
fn tonemap(color: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((color * (a * color + b)) / (color * (c * color + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn expand_range(color: vec3<f32>) -> vec3<f32> {
    let luminance = dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
    return color * (1.0 + HIGHLIGHT_GAIN * luminance * luminance);
}

// Linear in roughness rather than its square root: the fronts are polished to
// roughness 0.08, and they should keep the photograph's edges instead of
// blurring them away a few mip levels down.
fn environment_lod(roughness: f32, max_lod: f32) -> f32 {
    return clamp(roughness * max_lod, 0.0, max_lod);
}

fn background_sample(direction: vec3<f32>, lod: f32) -> vec3<f32> {
    let d = normalize(direction);
    let u = atan2(d.x, -d.z) / (2.0 * PI) + 0.5;
    let v = acos(clamp(d.y, -1.0, 1.0)) / PI;
    return textureSampleLevel(background_texture, environment_sampler, vec2<f32>(u, v), lod).rgb;
}

fn card_sample(direction: vec3<f32>, lod: f32) -> vec4<f32> {
    let forward = normalize(CARD_DIRECTION);
    let right = normalize(cross(vec3<f32>(0.0, 1.0, 0.0), forward));
    let up = cross(forward, right);

    let d = normalize(direction);
    let facing = dot(d, forward);
    if (facing <= 0.1) {
        return vec4<f32>(0.0);
    }

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

// The room around the kitchen as a mirror sees it. Rougher surfaces blur the
// bands between floor, wall and ceiling together.
fn room_environment(direction: vec3<f32>, roughness: f32) -> vec3<f32> {
    let d = normalize(direction);
    let blur = 0.08 + roughness * 0.5;
    let above = smoothstep(0.35 - blur, 0.35 + blur, d.y);
    let below = 1.0 - smoothstep(-0.05 - blur, -0.05 + blur, d.y);
    let wall_and_ceiling = mix(ROOM_WALL, ROOM_CEILING, above);
    return mix(wall_and_ceiling, ROOM_FLOOR, below);
}

// What the surroundings send back along `direction`: the room, with the skybox
// photograph over it when the images are there - most of all on polished metal.
fn environment(direction: vec3<f32>, roughness: f32, polish: f32) -> vec3<f32> {
    let room = room_environment(direction, roughness);
    if (globals.flags.y < 0.5) {
        return room;
    }

    var photo = background_sample(direction, environment_lod(roughness, globals.flags.z))
        * BACKGROUND_EXPOSURE;
    let card = card_sample(direction, environment_lod(roughness, globals.flags.w));
    photo = expand_range(mix(photo, card.rgb * CARD_EXPOSURE, card.a));

    let luminance = dot(photo, vec3<f32>(0.2126, 0.7152, 0.0722));
    photo = mix(vec3<f32>(luminance), photo, PHOTO_SATURATION);
    return mix(room, photo, mix(PHOTO_REFLECTION, POLISHED_PHOTO_REFLECTION, polish));
}

// 1.0 on the edges of the slot's box, fading to 0 a strip's width away. An
// edge is where two faces meet, so it is the middle one of the three distances
// to the nearest face on each axis that has to be small.
fn edge_glow(position: vec3<f32>) -> f32 {
    let size = placement.bounds_max.xyz - placement.bounds_min.xyz;
    if (min(size.x, min(size.y, size.z)) <= 0.0) {
        return 0.0;
    }

    let d = min(position - placement.bounds_min.xyz, placement.bounds_max.xyz - position);
    let a = abs(d.x);
    let b = abs(d.y);
    let c = abs(d.z);
    let middle = max(min(a, b), min(max(a, b), c));
    return 1.0 - smoothstep(LED_EDGE_WIDTH * 0.3, LED_EDGE_WIDTH, middle);
}

// The LED light a lit-up slot throws onto the floor around its foot and the wall
// behind it, as a fraction of full strength.
fn led_spill(position: vec3<f32>, surface: f32) -> f32 {
    let glow = globals.hover_min.w;
    if (glow <= 0.0) {
        return 0.0;
    }
    let low = globals.hover_min.xyz;
    let high = globals.hover_max.xyz;

    if (abs(surface - SURFACE_FLOOR) < 0.5) {
        let distance = length(vec2<f32>(
            outside(position.x, low.x, high.x),
            outside(position.z, low.z, high.z),
        ));
        return glow * 0.9 * (1.0 - smoothstep(0.0, 0.45, distance));
    }

    if (abs(surface - SURFACE_WALL) < 0.5) {
        let distance = length(vec2<f32>(
            outside(position.x, low.x, high.x),
            outside(position.y, low.y, high.y),
        ));
        return glow * 0.7 * (1.0 - smoothstep(0.0, 0.4, distance));
    }

    return 0.0;
}

// How far `value` lies outside `low..high`, or 0 inside it.
fn outside(value: f32, low: f32, high: f32) -> f32 {
    return max(max(low - value, value - high), 0.0);
}

// A soft contact shadow standing the modules on the floor and against the wall:
// darkest right up against the run, gone within a hand's width. Also darkens the
// corner where the floor meets the wall, which no light in the scene reaches.
fn contact_occlusion(position: vec3<f32>, surface: f32) -> f32 {
    let run_min = globals.run_min;
    let run_max = globals.run_max;

    if (abs(surface - SURFACE_FLOOR) < 0.5) {
        let beside = outside(position.x, run_min.x, run_max.x);
        let in_front = outside(position.z, run_min.z, run_max.z);
        let footprint = 1.0 - smoothstep(0.0, 0.3, length(vec2<f32>(beside, in_front)));
        // Under the modules there is only the toe-kick gap for light to get in.
        let under = select(0.0, 0.25, beside <= 0.0 && in_front <= 0.0);
        let along_wall = 1.0 - smoothstep(0.0, 0.2, outside(position.x, -run_min.w, run_min.w));
        let corner = (1.0 - smoothstep(0.0, 0.35, position.z - run_min.z)) * along_wall;
        return clamp(1.0 - 0.4 * footprint - under - 0.2 * corner, 0.0, 1.0);
    }

    if (abs(surface - SURFACE_WALL) < 0.5) {
        let beside = outside(position.x, run_min.x, run_max.x);
        let above = max(position.y - run_max.y, 0.0);
        let behind = 1.0 - smoothstep(0.0, 0.25, length(vec2<f32>(beside, above)));
        let corner = 1.0 - smoothstep(0.0, 0.3, position.y);
        return clamp(1.0 - 0.3 * behind - 0.2 * corner, 0.0, 1.0);
    }

    return 1.0;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    var albedo = material.base_color.rgb;
    if (material.params.z > 0.5) {
        albedo = albedo * textureSample(base_color_texture, base_color_sampler, input.uv).rgb;
    }

    let metallic = material.params.x;
    let roughness = clamp(material.params.y, 0.045, 1.0);

    // The assets are double sided, and so is the lamp shade: light whichever
    // side faces the camera.
    var normal = normalize(input.normal);
    let view_direction = normalize(globals.camera_position.xyz - input.world_position);
    if (dot(normal, view_direction) < 0.0) {
        normal = -normal;
    }

    let n_dot_v = max(dot(normal, view_direction), 1e-4);
    let f0 = mix(vec3<f32>(0.04), albedo, metallic);
    let occlusion = contact_occlusion(input.world_position, material.emissive.w);

    var radiance = vec3<f32>(0.0);
    for (var i = 0; i < 3; i = i + 1) {
        let to_light = globals.light_positions[i].xyz - input.world_position;
        let distance = max(length(to_light), 1e-3);
        let light_direction = to_light / distance;
        let n_dot_l = max(dot(normal, light_direction), 0.0);
        if (n_dot_l <= 0.0) {
            continue;
        }

        // Inverse square, windowed to zero at the light's range, and only
        // below the shade: a pendant throws its light down, not at the ceiling.
        let range = globals.light_positions[i].w;
        let window = clamp(1.0 - pow(distance / range, 4.0), 0.0, 1.0);
        let below_shade = smoothstep(-0.2, 0.3, light_direction.y);
        let attenuation = window * window * below_shade / max(distance * distance, 0.05);

        let half_vector = normalize(light_direction + view_direction);
        let n_dot_h = max(dot(normal, half_vector), 0.0);
        let distribution = distribution_ggx(n_dot_h, roughness);
        let geometry = geometry_smith(n_dot_v, n_dot_l, roughness);
        let fresnel = fresnel_schlick(max(dot(half_vector, view_direction), 0.0), f0);

        let specular = distribution * geometry * fresnel / max(4.0 * n_dot_v * n_dot_l, 1e-4);
        let diffuse = (vec3<f32>(1.0) - fresnel) * (1.0 - metallic) * albedo / PI;

        radiance += (diffuse + specular) * globals.light_colors[i].rgb * attenuation * n_dot_l;
    }

    // Ambient diffuse: the room's bounce light, with some of the skybox's
    // colour when there is one.
    let hemisphere = mix(GROUND_AMBIENT, SKY_AMBIENT, normal.y * 0.5 + 0.5);
    let ambient = hemisphere * albedo * (1.0 - metallic);

    // Ambient specular: what the surface mirrors of the surroundings.
    let reflection = reflect(-view_direction, normal);
    let fresnel_ambient = f0 + (max(vec3<f32>(1.0 - roughness), f0) - f0)
        * pow(clamp(1.0 - n_dot_v, 0.0, 1.0), 5.0);
    let reflection_strength = mix(DIELECTRIC_REFLECTION, METAL_REFLECTION, metallic);
    // 1 for mirror-polished metal, fading out by a satin finish.
    let polish = metallic * (1.0 - smoothstep(0.05, 0.5, roughness));
    let ambient_specular = environment(reflection, roughness, polish) * fresnel_ambient
        * reflection_strength;

    var alpha = material.params.w;
    if (alpha < 1.0) {
        // Glass is most visible where it turns away from the viewer.
        let rim = pow(1.0 - n_dot_v, 3.0);
        alpha = clamp(mix(alpha, 1.0, rim), 0.0, 1.0);
    }

    // The hover glow: LED strips light up along the edges of the slot's box.
    // A placeholder's frosted glass fills with their light as well; a placed
    // module only takes on a faint cool tint, so it still reads as its finish.
    var emissive = material.emissive.rgb;
    let glow = placement.highlight.x;
    let edge = edge_glow(input.world_position);
    let rim = pow(1.0 - n_dot_v, 2.0);
    if (placement.highlight.y > 0.5) {
        let strip = edge * mix(PLACEHOLDER_IDLE_EDGE, 3.2, glow);
        emissive += LED_COLOR * (strip + glow * (0.5 + 1.2 * rim));
        alpha = max(mix(alpha, max(alpha, 0.62), glow), edge * mix(0.5, 1.0, glow));
    } else {
        emissive += LED_COLOR * glow * (0.08 + 0.4 * rim + 2.2 * edge);
    }
    emissive += LED_COLOR * albedo * led_spill(input.world_position, material.emissive.w);

    var color = tonemap((radiance + ambient) * occlusion + ambient_specular + emissive);
    if (globals.flags.x > 0.5) {
        color = pow(color, vec3<f32>(1.0 / 2.2));
    }

    return vec4<f32>(color, alpha);
}
