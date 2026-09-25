#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

#include "shared.frag"

in float v_transitionDelta;
uniform vec4 u_transitionColor;

out vec4 fragColor;

void main()
{
    if (v_transitionDelta <= 0.0)
        discard;

    fragColor.rgb = u_transitionColor.rgb;
    fragColor.a = v_transitionDelta;
}