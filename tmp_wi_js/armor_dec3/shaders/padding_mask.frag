#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

in vec2 v_texCoord;
out vec4 fragColor;

#include "shared.frag"

void main()
{
    float distsq = v_texCoord.x * v_texCoord.x + v_texCoord.y * v_texCoord.y;
    float alpha = 1.0 - smoothstep(0.2, 0.3, distsq);
    fragColor = vec4(0.0, 0.0, 0.0, alpha + hash12(v_texCoord)/255.0);
}