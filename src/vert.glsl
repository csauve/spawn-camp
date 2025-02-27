#version 460

layout(location = 0) in vec2 lm_uv;
layout(location = 1) in vec3 world_pos;

layout(location = 0) out vec3 v_world_pos;

void main() {
    v_world_pos = world_pos;
    gl_Position = vec4(2.0 * lm_uv - 1.0, 0.0, 1.0);
}