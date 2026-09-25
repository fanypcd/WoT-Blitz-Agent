#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

#include "shared.frag"

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
#if !defined(DEPTH_PASS)
uniform vec3 u_ambientColor;
uniform sampler2D u_diffuseTexture;
#endif

uniform sampler2D u_normalmapTexture;
#ifndef LEGACY_SHADER
uniform float u_alphaRef;
#endif

#if defined(LIGHTMAP)
uniform sampler2D u_lightmapTexture;
#endif

#if defined(LIGHTING)

#if (DIRECTIONAL_LIGHT_COUNT > 0)
uniform vec3 u_directionalLightColor[DIRECTIONAL_LIGHT_COUNT];
#if !defined(BUMPED)
uniform vec3 u_directionalLightDirection[DIRECTIONAL_LIGHT_COUNT];
#endif
#endif

#if (POINT_LIGHT_COUNT > 0)
uniform vec3 u_pointLightColor[POINT_LIGHT_COUNT];
uniform vec3 u_pointLightPosition[POINT_LIGHT_COUNT];
uniform float u_pointLightRangeInverse[POINT_LIGHT_COUNT];
#endif

#if (SPOT_LIGHT_COUNT > 0)
uniform vec3 u_spotLightColor[SPOT_LIGHT_COUNT];
uniform float u_spotLightRangeInverse[SPOT_LIGHT_COUNT];
uniform float u_spotLightInnerAngleCos[SPOT_LIGHT_COUNT];
uniform float u_spotLightOuterAngleCos[SPOT_LIGHT_COUNT];
#if !defined(BUMPED)
uniform vec3 u_spotLightDirection[SPOT_LIGHT_COUNT];
#endif
#endif

#if defined(SPECULAR)
uniform sampler2D u_skySpecularTexture;
uniform sampler2D u_metallicGlossTexture;
#endif

#if defined(DETAIL_MAP)
uniform sampler2D u_detailTexture;
uniform vec2 u_detailTiling;
#endif

#if defined(VERSION_HD) && defined(SPECULAR)
uniform float u_cloudyFactor;
#endif

#if defined(VERSION_HD) && defined(AMBIENT_OCCLUSION) && !defined(LEGACY_SHADER)
uniform sampler2D u_aoTexture;
uniform sampler2D u_camoTexture;
uniform vec4 u_camoTiling;
uniform vec4 u_camoPalette0;
uniform vec4 u_camoPalette1;
uniform vec4 u_camoPalette2;
uniform vec4 u_camoPalette3;
uniform vec4 u_paintColor;
uniform float u_paintFade;
//uniform vec2 u_paintGlossMetallic;
#endif

#endif  // LIGHTING

#if defined(MODULATE_COLOR)
uniform vec4 u_modulateColor;
#endif

#if defined(MODULATE_ALPHA)
uniform float u_modulateAlpha;
#endif

#if defined(VERSION_HD) && defined(TRANSITION_FX)
in float v_transitionDelta;
#endif

#if defined(USE_SHADOWS)
uniform sampler2D u_shadowMap;
uniform mat4 u_shadowMapViewProjection[1];
in vec4 v_worldPoint;
#endif

#ifdef HITSKIN

#ifdef HOLLOW_CHARGE
uniform vec2 u_depthConstants;
uniform float u_piercingPowerLossFactor;
uniform sampler2D u_firstSpacedArmorTexture;
#endif

#ifdef MODULES
uniform sampler2D u_modulesColorTexture;
uniform sampler2D u_modulesTexture;
#endif

uniform sampler2D u_piercingTexture;
uniform sampler2D u_ricochetTexture;
in vec3 v_screenCoord;

uniform vec4 u_platform;

float calcChance(vec3 sampleCoord)
{
    vec4 data = textureProj(u_piercingTexture, sampleCoord);

    if (data.a <= 0.0)
        return 0.0; // collision model is not hit

    if (data.g > 0.0)
    {
        // ricochet, make a sampling from ricochet texture
        vec4 data2 = textureProj(u_ricochetTexture, sampleCoord);

        data2.r *= 1.3333; // 1.3333 is 1.0 / 0.75 (piercing power is lost by 25% after ricochet)

        // ricochet on blitz happens only on first impact, no need to check spaced armor thickness
        if (u_platform.x > 0.0)
            data.r = data2.r; 
        else
            data.r += data2.r;

        data.b = data2.b;
        data.g = data2.g;
    }

    if (data.g > 0.0)
        return 0.0; // ricochet

    float distToPrimaryArmor = data.b;
    if (distToPrimaryArmor == 0.0)
        return 0.0;

    float armorByMaxPiercing = data.r;

#ifdef HOLLOW_CHARGE
    float space = (data.b - data.a) * u_depthConstants.y;
    // first spaced plate uses full PP; remaining armor is inflated by gap loss
    float first = min(textureProj(u_firstSpacedArmorTexture, sampleCoord).r, armorByMaxPiercing);
    float denom = 1.0 - u_piercingPowerLossFactor * space;
    if (denom <= 0.0)
        return 0.0;
    armorByMaxPiercing = first + (armorByMaxPiercing - first) / denom;
#endif

    return getPiercingChance( armorByMaxPiercing, u_platform.w, 1.0, u_platform );
}

#ifdef MODULES
vec3 applyDamage(vec3 color)
{
    float chance = calcChance(v_screenCoord);
    if (chance < 0.0)
        return vec3(0.25, 0.25, 0.25);

    vec3 penetrationColor = mix(color, vec3(1.0, 1.0, 1.0), chance);
    vec4 modules = texture(u_modulesTexture, v_screenCoord.xy / v_screenCoord.z);
    // r - type of internal module
    // g - distance factor
    // b - shading
    // a - type of external module

    if (modules.r > 0.0 && chance > 0.0)
    {
        vec3 moduleColor = texture(u_modulesColorTexture, vec2(modules.r, 0.5)).rgb;

        float gray = 1.0;//grayscale(moduleColor);
        moduleColor = mix(moduleColor, vec3(gray, gray, gray), modules.g) * modules.b;
        penetrationColor = mix(penetrationColor, moduleColor, chance);
    }

    if (modules.a > 0.0)
    {
        vec3 moduleColor = texture(u_modulesColorTexture, vec2(modules.a, 0.5)).rgb;

        // always draw gun and observation device on top of internal modules
        if (modules.a > 6.0 / 16.0)
            penetrationColor = moduleColor;
        else
            penetrationColor = mix(penetrationColor, moduleColor, 1.0 - chance);
    }

    return penetrationColor;
}
#endif  // MODULES

#endif // HITSKIN

///////////////////////////////////////////////////////////
// Variables
vec4 _baseColor;

///////////////////////////////////////////////////////////
// Varyings
in vec2 v_texCoord;

#if defined(LIGHTMAP)
in vec2 v_texCoord1;
#endif

#if defined(LIGHTING)

#if !defined(BUMPED)
in vec3 v_normalVector;
#endif

#if defined(BUMPED) && (DIRECTIONAL_LIGHT_COUNT > 0)
in vec3 v_directionalLightDirection[DIRECTIONAL_LIGHT_COUNT];
#endif

#if (POINT_LIGHT_COUNT > 0)
in vec3 v_vertexToPointLightDirection[POINT_LIGHT_COUNT];
#endif

#if (SPOT_LIGHT_COUNT > 0)
in vec3 v_vertexToSpotLightDirection[SPOT_LIGHT_COUNT];
#if defined(BUMPED)
in vec3 v_spotLightDirection[SPOT_LIGHT_COUNT];
#endif
#endif

in vec3 v_upDirection;

#if defined(SPECULAR)
in vec3 v_cameraDirection; 
#endif

#if defined(INTERIOR)
uniform float u_interiorLuminosity;
#endif

#include "lighting_textured.frag"
#endif

#if defined(CLIP_PLANE)
in float v_clipDistance;
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

#if defined(DEPTH_PASS)

    _baseColor = texture(u_normalmapTexture, v_texCoord);
#if defined(VERSION_HD)
    if (_baseColor.r < u_alphaRef)
#else
    if (_baseColor.b < u_alphaRef)
#endif
        discard;
    fragColor = vec4(0.0);

#else

    _baseColor = texture(u_diffuseTexture, v_texCoord);

    #if defined(MODULATE_COLOR)
    _baseColor *= u_modulateColor;
    #endif

#if defined(HITSKIN)
#ifdef MODULES
    _baseColor.rgb = applyDamage(_baseColor.rgb);
#else
    float chance = calcChance(v_screenCoord);
    _baseColor.rgb = mix(vec3(1.0, 0.0, 0.0), _baseColor.rgb, chance);
#endif
#endif

    #if defined(TEXTURE_DISCARD_ALPHA)
    if (_baseColor.a < 0.5)
        discard;
    #endif

    #if defined(LIGHTING)

#if defined(SIMPLE_LIGHTING)
    vec3 ambientColor = _baseColor.rgb * u_ambientColor;
    vec3 litPixel = getLitPixel();
    _baseColor.rgb = ambientColor + litPixel;
#else
    _baseColor = srgb2lin(_baseColor);    // convert to linear space
    _baseColor.rgb = getLitPixel();
    _baseColor = lin2srgb(_baseColor);
#endif

#if defined(INTERIOR)
    float gray = grayscale(_baseColor.rgb);
    _baseColor.rgb = mix(vec3(gray + u_interiorLuminosity), _baseColor.rgb, 0.2);
#endif

    #endif

    fragColor = _baseColor;

    #if defined(LIGHTMAP)
    vec4 lightColor = texture(u_lightmapTexture, v_texCoord1);
    fragColor.rgb *= lightColor.rgb;
    #endif

    #if defined(MODULATE_ALPHA)
    fragColor.a *= u_modulateAlpha;
    #endif

#endif

#if defined(VERSION_HD) && defined(TRANSITION_FX)
    fragColor.a = v_transitionDelta;
#endif
}