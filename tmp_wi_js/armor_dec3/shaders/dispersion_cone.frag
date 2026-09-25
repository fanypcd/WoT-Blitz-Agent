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

///////////////////////////////////////////////////////////
// Variables

in float v_direction;
in float v_transparency;

out vec4 fragColor;

#include "shared.frag"

void main()
{
    fragColor = vec4(u_color.xyz, u_alpha * (1.0 - v_transparency * 0.8));//u_alpha * (0.7 - v_direction * 0.5));
}