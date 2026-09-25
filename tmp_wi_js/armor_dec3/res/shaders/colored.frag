#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

#ifndef DIRECTIONAL_LIGHT_COUNT
#define DIRECTIONAL_LIGHT_COUNT 0
#endif
#ifndef SPOT_LIGHT_COUNT
#define SPOT_LIGHT_COUNT 0
#endif
#ifndef POINT_LIGHT_COUNT
#define POINT_LIGHT_COUNT 0
#endif
#if (DIRECTIONAL_LIGHT_COUNT > 0) || (POINT_LIGHT_COUNT > 0) || (SPOT_LIGHT_COUNT > 0)
#define LIGHTING
#endif

///////////////////////////////////////////////////////////
// Uniforms
uniform vec3 u_ambientColor;
uniform vec4 u_diffuseColor;

#if defined(LIGHTMAP)
uniform sampler2D u_lightmapTexture;
#endif

#if defined(LIGHTING)

#if (DIRECTIONAL_LIGHT_COUNT > 0)
uniform vec3 u_directionalLightColor[DIRECTIONAL_LIGHT_COUNT];
uniform vec3 u_directionalLightDirection[DIRECTIONAL_LIGHT_COUNT];
#endif

#if (POINT_LIGHT_COUNT > 0)
uniform vec3 u_pointLightColor[POINT_LIGHT_COUNT];
uniform vec3 u_pointLightPosition[POINT_LIGHT_COUNT];
uniform float u_pointLightRangeInverse[POINT_LIGHT_COUNT];
#endif

#if (SPOT_LIGHT_COUNT > 0)
uniform vec3 u_spotLightColor[SPOT_LIGHT_COUNT];
uniform vec3 u_spotLightDirection[SPOT_LIGHT_COUNT];
uniform float u_spotLightRangeInverse[SPOT_LIGHT_COUNT];
uniform float u_spotLightInnerAngleCos[SPOT_LIGHT_COUNT];
uniform float u_spotLightOuterAngleCos[SPOT_LIGHT_COUNT];
#endif

#if defined(SPECULAR)
uniform float u_specularExponent;
#endif

#endif

#if defined(MODULATE_COLOR)
uniform vec4 u_modulateColor;
#endif

#if defined(MODULATE_ALPHA)
uniform float u_modulateAlpha;
#endif

///////////////////////////////////////////////////////////
// Variables
vec4 _baseColor;

///////////////////////////////////////////////////////////
// Varyings
#if defined(VERTEX_COLOR)
in vec3 v_color;
#endif

#if defined(LIGHTING)

in vec3 v_normalVector;

#if (POINT_LIGHT_COUNT > 0)
in vec3 v_vertexToPointLightDirection[POINT_LIGHT_COUNT];
#endif

#if (SPOT_LIGHT_COUNT > 0)
in vec3 v_vertexToSpotLightDirection[SPOT_LIGHT_COUNT];
#endif

#if defined(SPECULAR)
in vec3 v_cameraDirection; 
#endif

#include "lighting.frag"

#endif

#ifdef ENV_SATURATION
uniform float u_envSaturation[5];
#include "../../shaders/shared.frag"
#endif

out vec4 fragColor;

void main()
{
    #if defined(LIGHTING)

    #if defined(VERTEX_COLOR)
    _baseColor.rgb = v_color;
    #else
    _baseColor = u_diffuseColor;
    #endif
    
    fragColor.a = _baseColor.a;
    fragColor.rgb = getLitPixel();
    
    #else
    
    #if defined(VERTEX_COLOR)
    fragColor.rgb = v_color;
    fragColor.a = 1.0;
    #else
    fragColor = u_diffuseColor;
    #endif
    
    #endif

    #if defined(LIGHTMAP)
    vec4 lightColor = texture(u_lightmapTexture, v_texCoord1);
    fragColor.rgb *= lightColor.rgb;
    #endif

    #if defined(MODULATE_COLOR)
    fragColor *= u_modulateColor;
    #endif

    #if defined(MODULATE_ALPHA)
    fragColor.a *= u_modulateAlpha;
    #endif

#ifdef ENV_SATURATION
    fragColor.rgb = mix(vec3(grayscale(fragColor.rgb)), fragColor.rgb, u_envSaturation[0]);
#endif
}