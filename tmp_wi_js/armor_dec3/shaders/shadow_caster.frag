#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif


#include "shared.frag"


#if defined(CLIP_PLANE)
in float v_clipDistance;
#endif

#if defined(CLIP_INTERIOR) || defined(ALPHA_TEST)
in vec2 v_texCoord;
#endif

#if defined(ALPHA_TEST)
uniform sampler2D u_normalmapTexture;
uniform float u_alphaRef;
#endif

out vec4 fragColor;

void main()
{
#if defined(CLIP_PLANE)
    if (v_clipDistance < 0.0)
        discard;
#endif

#if defined(CLIP_INTERIOR)
    if (v_texCoord.x < 0.0)
        discard;
#endif

#if defined(ALPHA_TEST)
    vec4 normalMapData = texture(u_normalmapTexture, v_texCoord);
    #if defined(VERSION_HD)
    if (normalMapData.r < u_alphaRef)
        discard;
    #else
    if (normalMapData.b < u_alphaRef)
        discard;
    #endif
#endif

    float depthValue = gl_FragCoord.z;
    fragColor = encodeFloatRGBA(depthValue);
}