#version 460

layout(location = 0) in vec3 v_world_pos;
layout(location = 1) in vec3 v_world_normal;

layout(location = 0) out vec4 f_color;

struct SpawnData {
    vec3 world_pos;
};

layout(set = 0, binding = 0, std140) uniform UniformData {
    uint spawn_count;
    SpawnData spawns[256];
} data;

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

void main() {
//    vec3 sun = normalize(vec3(0.0, 1.0, 1.0));
//    float direct_light = clamp(dot(v_world_normal, sun), 0.0, 1.0);
//    float light = direct_light + 0.2;

    vec3 spawn_color = is_random() ? vec3(1.0, 0.5, 0.5) : vec3(1.0, 1.0, 1.0);
    f_color = vec4(spawn_color, 1.0);
}
