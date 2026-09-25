#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

uniform sampler2D u_shadowMap;
uniform mat4 u_shadowMapViewProjection0;

const float radius = 3.0;

in vec4 v_worldPosition;
in vec2 v_texCoord;

out vec4 fragColor;

#include "shared.frag"

void main()
{
    vec4 shadowMapPlanePoint = u_shadowMapViewProjection0 * v_worldPosition;
    vec2 shadowMapUV = shadowMapPlanePoint.xy / shadowMapPlanePoint.w * 0.5 + vec2(0.5, 0.5);
    vec4 depthSample = texture(u_shadowMap, shadowMapUV);

    vec4 color0 = vec4(0.0, 0.0, 0.0, 0.0);

    if (depthSample.r * depthSample.g * depthSample.b * depthSample.a < 1.0)    // check that depth buffer has values other than 1.0
        color0.a = 0.5;

    fragColor.rgb = color0.rgb;
    fragColor.a = color0.a;
}