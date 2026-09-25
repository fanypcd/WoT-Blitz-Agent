#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

#include "lighting_header.frag"

///////////////////////////////////////////////////////////
// Uniforms

///////////////////////////////////////////////////////////
// Varyings

in vec2 v_texCoord;
uniform vec4 u_color;

out vec4 fragColor;

#include "shared.frag"

void main()
{
    float fillFunc = (floor(mod((v_texCoord.x - v_texCoord.y) * 16.0, 2.0)) * 0.2 + 0.8);
    _baseColor = u_color * fillFunc;

#ifdef LIGHTING

    _baseColor = srgb2lin(_baseColor);

    vec3 ambientColor = _baseColor.rgb * u_ambientColor;
    vec3 litPixel = getLitPixel();
    vec4 res = lin2srgb(vec4(ambientColor + litPixel, 1.0));
    fragColor = res;

#else

    fragColor = _baseColor;

#endif
}