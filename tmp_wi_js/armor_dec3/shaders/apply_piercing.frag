#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

#include "lighting_header.frag"

#if defined(RAMMING)
uniform float u_potentialRammingDamage;
uniform sampler2D u_damageRamp;
uniform sampler2D u_damageTexture;

#elif defined(HEDAMAGE)

uniform sampler2D u_damageRamp;
uniform sampler2D u_damageTexture;

#elif defined(HEDAMAGE_MODERN)

uniform sampler2D u_piercingTexture;
uniform sampler2D u_damageRamp;
uniform vec4 u_platform;

#else // HEDAMAGE

uniform vec4 u_platform;

#ifdef HOLLOW_CHARGE
uniform vec2 u_depthConstants;
uniform float u_piercingPowerLossFactor;
uniform sampler2D u_firstSpacedArmorTexture;
#endif

uniform sampler2D u_piercingTexture;
uniform sampler2D u_ricochetTexture;
#ifdef LEGACY
uniform sampler2D u_penetrationRamp;
#else
uniform sampler2D u_modulesColorTexture;
uniform sampler2D u_modulesTexture;
uniform vec4 u_modulateColor;
#endif

#if defined(USE_SHADOWS)
uniform sampler2D u_shadowMap;
uniform mat4 u_shadowMapViewProjection;
in vec4 v_worldPoint;
#endif

#endif // RAMMING

in vec3 v_screenCoord;

out vec4 fragColor;

#include "shared.frag"

#if defined(RAMMING)

vec3 applyDamage()
{
    vec2 normalizedScreenCoord = v_screenCoord.xy / v_screenCoord.z;
    vec4 data = texture(u_damageTexture, normalizedScreenCoord);

    if (data.a <= 0.0)
        return vec3(0.25, 0.25, 0.25);

    vec3 res;

    float armor = data.r * 768.0;
    float damageHE = 1.0 - armor / u_potentialRammingDamage;

    vec2 direction = vec2(v_normalVector.x, v_normalVector.z);
    float directionFactor = v_normalVector.z / sqrt(dot(direction, direction));

    float damageToVehicle = damageHE * directionFactor;
    res = texture(u_damageRamp, vec2(damageToVehicle,0.5)).rgb;

    return res;
}

#elif defined(HEDAMAGE)

vec3 applyDamage()
{
    vec2 normalizedScreenCoord = v_screenCoord.xy / v_screenCoord.z;
    vec4 data = texture(u_damageTexture, normalizedScreenCoord);

    vec3 res = texture(u_damageRamp, vec2(data.g,0.5)).rgb + data.rrr;
    return res;
}

#elif defined(HEDAMAGE_MODERN)

vec3 applyDamage()
{
    vec4 data = textureProj(u_piercingTexture, v_screenCoord);

    if (data.g == 0.0)
    {
        // there is no primary armor behind
        return vec3(0.25, 0.25, 0.25);
    }

    // check chance to pen spaced armor
    float chance = getPiercingChance(3.0 * data.b - 2.0 * data.a, u_platform.w, 1.0, u_platform);
    if (chance <= 0.0)
    {
        // can't penetrate spaced armor, zero damage
        return vec3(0.25, 0.25, 0.25);
    }

    // total chance
    chance = getPiercingChance(data.r, u_platform.w, 1.0, u_platform);
    vec3 res = texture(u_damageRamp, vec2(data.g, 0.5)).rgb + vec3(chance);
    return res;
}

#else

vec2 calcChance(vec3 sampleCoord)
{
    vec4 data = textureProj(u_piercingTexture, sampleCoord);

    if (data.a <= 0.0)
        return vec2(-1.0); // collision model is not hit

    if (data.g > 0.0)
    {
        // ricochet, make a sampling from ricochet texture
        vec4 data2 = textureProj(u_ricochetTexture, sampleCoord);

        data2.r *= 1.3333; // 1.3333 is 1.0 / 0.75 (piercing power is lost by 25% after ricochet)

        // ricochet on blitz happens only on first impact, no need to check spaced armor thickness
        if (u_platform.x > 0.0 || data.g > 0.5) // 0.5 - ricochet on primary armor, 1.0 - ricochet on spaced armor
            data.r = data2.r; 
        else
            data.r += data2.r;  // ricochet on primary armor, add spaced armor

        data.b = data2.b;
        if (data2.g > 0.0)
            return vec2(0.0, data.g); // ricochet
    }

    float distToPrimaryArmor = data.b;
    if (distToPrimaryArmor == 0.0)
        return vec2(0.0, data.g);

    float armorByMaxPiercing = data.r;

#ifdef HOLLOW_CHARGE
    float space = (data.b - data.a) * u_depthConstants.y;
    // first spaced plate uses full PP; remaining armor is inflated by gap loss
    float first = min(textureProj(u_firstSpacedArmorTexture, sampleCoord).r, armorByMaxPiercing);
    float denom = 1.0 - u_piercingPowerLossFactor * space;
    if (denom <= 0.0)
        return vec2(0.0, data.g);
    armorByMaxPiercing = first + (armorByMaxPiercing - first) / denom;
#endif

    return vec2(getPiercingChance( armorByMaxPiercing, u_platform.w, 1.0, u_platform ), data.g);
}

vec3 applyDamage()
{
    vec2 chance = calcChance(v_screenCoord);
    if (chance.x < 0.0)
        return vec3(0.25, 0.25, 0.25);

#if defined(LEGACY)

    vec3 penetrationColor = texture(u_penetrationRamp, vec2(chance.x, 0.5)).rgb;
    if (false)//chance.y > 0.0)
    {
        penetrationColor = rgb2hsv(penetrationColor);
        penetrationColor.x -= 0.05;
        penetrationColor = hsv2rgb(penetrationColor);
    }

#else 

    vec3 penetrationColor = mix(u_modulateColor.rgb, vec3(1.0, 1.0, 1.0), chance.x);
    vec4 modules = texture(u_modulesTexture, v_screenCoord.xy / v_screenCoord.z);
    // r - type of internal module
    // g - distance factor
    // b - shading
    // a - type of external module

    if (modules.r > 0.0 && chance.x > 0.0)
    {
        vec3 moduleColor = texture(u_modulesColorTexture, vec2(modules.r, 0.5)).rgb;

        float gray = 1.0;//grayscale(moduleColor);
        moduleColor = mix(moduleColor, vec3(gray, gray, gray), modules.g) * modules.b;
        penetrationColor = mix(penetrationColor, moduleColor, chance.x);
    }

    if (modules.a > 0.0)
    {
        vec3 moduleColor = texture(u_modulesColorTexture, vec2(modules.a, 0.5)).rgb;

        // always draw gun and observation device on top of internal modules
        if (modules.a > 6.0 / 16.0)
            penetrationColor = moduleColor;
        else
            penetrationColor = mix(penetrationColor, moduleColor, 1.0 - chance.x);
    }
#endif

    return penetrationColor;

}

#endif // RAMMING

void main()
{
    _baseColor.rgb = applyDamage();
    _baseColor.a = 1.0;
    vec4 res; 
    res.a = _baseColor.a;

    #if defined(LIGHTING)

    _baseColor = srgb2lin(_baseColor);
    vec3 ambientColor = _baseColor.rgb * u_ambientColor;
    vec3 litPixel = getLitPixel();

#if defined(USE_SHADOWS)
    vec4 shadowMapPoint = u_shadowMapViewProjection * v_worldPoint;
    vec2 shadowMapUV = shadowMapPoint.xy / shadowMapPoint.w * 0.5 + 0.5;
    float shadowMapDepth = texture(u_shadowMap, shadowMapUV).r;
    float testDepth = shadowMapPoint.z / shadowMapPoint.w;

    if (testDepth > shadowMapDepth + 0.01)
        litPixel *= 0.2;
#endif

    res.rgb = ambientColor + litPixel;
    res = lin2srgb(res);

    #else

    res.rgb = _baseColor.rgb;

    #endif

    fragColor.rgb = res.rgb;// + hash32(v_screenCoord.xy) / 255.0;
    fragColor.a = 1.0;
}