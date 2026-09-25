#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

#include "lighting_header.frag"

///////////////////////////////////////////////////////////
// Uniforms
uniform float u_maxDistance;
uniform vec2 u_depthConstants;
uniform float u_moduleType;
uniform sampler2D u_piercingTexture;

///////////////////////////////////////////////////////////
// Varyings
in float v_viewDistance; 
in vec3 v_screenCoord;

out vec4 fragColor;

#include "shared.frag"

void main()
{
    vec4 piercingSample = texture(u_piercingTexture, v_screenCoord.xy / v_screenCoord.z);
    if (piercingSample.b <= 0.0)
        discard;
    if (piercingSample.g > 0.0)
        discard;

    float distanceToModule = (v_viewDistance - piercingSample.b) * u_depthConstants.y - u_maxDistance;
    float distanceFactor = clamp((distanceToModule + 0.05) * 10.0, 0.0, 1.0);

    _baseColor = vec4(1.0, 1.0, 1.0, 1.0);

    #if defined(LIGHTING)
    _baseColor.rgb = u_ambientColor + getLitPixel() * (1.0 - 0.8 * distanceFactor);
    #endif

    fragColor = vec4(u_moduleType, distanceFactor, _baseColor.r, 0.0);
}