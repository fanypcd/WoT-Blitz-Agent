#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

#if defined(RAMMING)
uniform vec2 u_invTexSize;
#else
uniform float u_damage;
uniform float u_explosionRadius;
const float u_antifragmentationLiningFactor = 1.0;
#endif

uniform vec2 u_depthConstants;
uniform vec2 u_depthConstantsHE;
uniform mat4 u_inverseViewProjectionMatrix;
uniform mat4 u_heCameraViewProjectionMatrix;
uniform mat4 u_heCameraViewMatrix;

uniform sampler2D u_piercingTexture;
uniform sampler2D u_oldDamageTexture;
in vec2 v_texCoord;

uniform vec2 u_cameraPlanes;

out vec4 fragColor;

#include "shared.frag"

#if !defined(RAMMING)
float calcDamage( vec2 texCoord, float depthTest )
{
    vec4 newData = texture( u_piercingTexture, texCoord );

    if (newData.b == 0.0)
        return 0.0;

    float distance = newData.b * u_depthConstantsHE.y + u_depthConstantsHE.x + depthTest;
    if (distance < 0.0)
        return 0.0;

    float armor = newData.r * 768.0;
    if (distance >= u_explosionRadius)
        return 0.0;

    float damageInitial = u_damage;
    float damageHE = u_damage * (1.0 - distance / u_explosionRadius) - armor * 1.3 * u_antifragmentationLiningFactor;
    if (damageHE <= 0.0)
        return 0.0;

    return damageHE / damageInitial;
}
#endif

vec4 unprojectPoint( vec2 texCoord )
{
    vec4 damageData = texture( u_oldDamageTexture, texCoord );

#if defined(RAMMING)
    float firstHitDistance = damageData.a;
#else
    if (damageData.r == 1.0)
        return damageData;
    float firstHitDistance = damageData.b;
#endif

    if (firstHitDistance == 0.0)
        return damageData;
    
    float distance = firstHitDistance * u_depthConstants.y + u_depthConstants.x - u_depthConstants.y / 64.0;
    vec4 worldPoint = u_inverseViewProjectionMatrix * vec4(texCoord.xy * 2.0 - 1.0, u_cameraPlanes.x + u_cameraPlanes.y / distance, 1.0);
    
    vec4 heCameraProjectedPoint = u_heCameraViewProjectionMatrix * worldPoint;
    vec2 heCameraTexCoord = heCameraProjectedPoint.xy / heCameraProjectedPoint.w * 0.5 + 0.5;
    vec4 heCameraTestPoint = u_heCameraViewMatrix * worldPoint;
    float depthValue = heCameraTestPoint.z / heCameraTestPoint.w;

#if defined(RAMMING)
    float totalArmor = 0.0;
    float overrideArmor = damageData.b;

    // PCF filtering

    //*
    for(float ofsx = -0.5; ofsx < 1.0; ofsx += 1.0)
        for(float ofsy = -0.5; ofsy < 1.0; ofsy += 1.0)
        {
            vec4 newData = texture(u_piercingTexture, heCameraTexCoord + vec2(ofsx, ofsy) * u_invTexSize);
            float armor = damageData.r;
            if (newData.b > 0.0)
            {
                if (newData.b * u_depthConstantsHE.y + u_depthConstantsHE.x + depthValue > 0.0)
                {
                    if (newData.r < armor || overrideArmor <= 0.0)
                    {
                        armor = newData.r;
                        overrideArmor = 1.0;
                    }
                }
            }
            totalArmor += 0.25 * armor;
        }
    //*/

    return vec4(totalArmor, damageData.g, overrideArmor, damageData.a);

#else
    float damageNormalized = calcDamage( heCameraTexCoord, depthValue );
    return vec4(damageData.r, max(damageNormalized, damageData.g), damageData.b, damageData.a);
#endif
}


void main( )
{
    vec4 originalData = unprojectPoint( v_texCoord );

    fragColor.r = originalData.r;
    fragColor.g = originalData.g;
    fragColor.b = originalData.b;
    fragColor.a = originalData.a;
}