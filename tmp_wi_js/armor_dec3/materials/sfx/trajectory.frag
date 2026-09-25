#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

#if defined(CROSS_SECTION)
in vec4 v_color;
#else
uniform vec4 u_color;
#endif
uniform float u_alpha;

out vec4 fragColor;

void main()
{
#if defined(CROSS_SECTION)
    fragColor = v_color;
#else
    fragColor = u_color;
#endif
    fragColor.a *= u_alpha;
}