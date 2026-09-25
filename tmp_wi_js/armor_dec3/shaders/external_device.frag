#version 300 es
precision mediump float;

#ifndef DEPTH_PASS
uniform float u_moduleType;
#endif

out vec4 fragColor;

void main()
{
#ifdef DEPTH_PASS
    fragColor = vec4(0.0, 0.0, 0.0, 0.0);
#else
    fragColor = vec4(0.0, 0.0, 0.0, u_moduleType);
#endif
}