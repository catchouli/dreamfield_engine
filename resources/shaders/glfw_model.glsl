#version 330 core

#define M_PI 3.1415926535897932384626433832795

layout (std140) uniform GlobalRenderParams
{
    float sim_time;
    mat4 mat_proj;
    mat4 mat_view;
};

layout (std140) uniform ModelRenderParams
{
    mat4 mat_model;
    mat3 mat_normal;
};

#ifdef BUILDING_VERTEX_SHADER

layout (location = 0) in vec3 in_pos;
layout (location = 1) in vec3 in_nrm;
layout (location = 3) in vec2 in_uv;

out vec3 var_nrm;
out vec2 var_uv;

void main() {
    var_nrm = mat_normal * in_nrm;
    var_uv = in_uv;
    gl_Position = mat_proj * mat_view * mat_model * vec4(in_pos.x, in_pos.y, in_pos.z, 1.0);
}

#endif

#ifdef BUILDING_FRAGMENT_SHADER

uniform sampler2D tex_base_color;

in vec3 var_nrm;
in vec2 var_uv;

out vec4 out_frag_color;

void main() {
    vec4 tex = texture(tex_base_color, var_uv);
    vec3 sun_dir = vec3(0.0, 0.0, 1.0);
    float dot = dot(sun_dir, var_nrm);
    out_frag_color = tex * vec4(dot, dot, dot, 1.0);
}

#endif