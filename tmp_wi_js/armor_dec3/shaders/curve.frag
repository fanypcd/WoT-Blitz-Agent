#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

///////////////////////////////////////////////////////////
// Uniforms
uniform vec4 u_color;
uniform float u_alpha;
uniform float u_alphaMod;

///////////////////////////////////////////////////////////
// Variables

///////////////////////////////////////////////////////////
// Varyings

in vec4 v_data;

out vec4 fragColor;

#include "shared.frag"

void main()
{
#if defined(VERTEX_ALPHA)
    fragColor = vec4(u_color.xyz, v_data.w * u_alpha * u_alphaMod);
#else
    fragColor = vec4(u_color.xyz, u_alpha * u_alphaMod);
#endif
}