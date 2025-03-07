#version 460

layout(location = 0) in vec2 lm_uv;
layout(location = 1) in vec3 world_pos;
layout(location = 2) in vec3 world_normal;

layout(location = 0) out vec3 v_world_pos;
layout(location = 1) out vec3 v_world_normal;
layout(location = 2) out vec2 v_lm_uv;

void main() {
    v_world_pos = world_pos;
    v_world_normal = world_normal;
    v_lm_uv = lm_uv;
    gl_Position = vec4(2.0 * lm_uv - 1.0, 0.0, 1.0);
}