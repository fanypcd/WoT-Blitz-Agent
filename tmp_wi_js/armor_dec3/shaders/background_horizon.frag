#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

in vec2 v_texCoord;
in vec4 v_color;

out vec4 fragColor;

#include "shared.frag"

void main()
{
    const vec3 color0 = vec3(0.38039, 0.38823, 0.45098);
    const vec3 color1 = vec3(0.4745, 0.53333, 0.58431);
    const vec3 color2 = vec3(0.67058, 0.71372, 0.7098);
    const vec3 color3 = vec3(0.72156, 0.74509, 0.72352);

    float t = v_texCoord.y;

    vec3 color = mix(color0, color1, smoothstep(0.0, 0.4, t));
    color = mix(color, color2, smoothstep(0.4, 0.94, t));
    color = mix(color, color3, smoothstep(0.94, 1.0, t));

    fragColor.rgb = color + hash32(v_texCoord)/255.0;
    fragColor.a = 1.0;
}