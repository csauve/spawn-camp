#version 460

layout(location = 0) in vec3 v_world_pos;
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
        float dist = distance(v_world_pos, data.spawns[i].world_pos);
        if (dist > 1.0 && dist < 6.0) {
            return false;
        }
    }
    return true;
}

void main() {
    vec3 spawn_color = is_random() ? vec3(0.0, 0.0, 0.0) : vec3(1.0, 1.0, 1.0);
    f_color = vec4(spawn_color, 1.0);
}
