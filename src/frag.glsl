#version 460

layout(location = 0) in vec3 v_world_pos;
layout(location = 1) in vec3 v_world_normal;
layout(location = 2) in vec2 v_lm_uv;

layout(location = 0) out vec4 f_color;

struct SpawnData {
    vec3 world_pos;
};

layout(set = 0, binding = 0, std140) uniform UniformData {
    uint spawn_count;
    SpawnData spawns[256];
    vec4 randoms_color;
    uint blend_mode;
    uint walkable_only;
} data;
layout(set = 0, binding = 1) uniform sampler s;
layout(set = 0, binding = 2) uniform texture2D lm_page;

bool is_random() {
    for (uint i = 0; i < data.spawn_count; i++) {
        vec3 diff = v_world_pos - data.spawns[i].world_pos;
        float dist_squared = dot(diff, diff);
        if (dist_squared > 1.0 && dist_squared < 36.0) {
            return false;
        }
    }
    return true;
}

bool mask() {
    return data.walkable_only == 0 || dot(v_world_normal, vec3(0.0, 0.0, 1.0)) > 0.5;
}

void main() {
    if (!is_random() || !mask()) {
        discard;
    }

    vec3 lm = texture(sampler2D(lm_page, s), v_lm_uv).rgb;

    //normal
    vec3 blended = data.randoms_color.rgb;
    if (data.blend_mode == 1) {
        //multiply
        blended = lm * vec3(data.randoms_color.rgb);
    }

    lm = mix(lm, blended, data.randoms_color.a);
    f_color = vec4(lm, 1.0);
}
