#version 300 es
precision mediump float;

uniform float u_piercingConstants;    // piercingPower
uniform float u_damage;
uniform float u_explosionRadius;
uniform vec2 u_depthConstants;
const float u_antifragmentationLiningFactor = 1.0;
uniform vec4 u_platform;

uniform sampler2D u_piercingTexture;
in vec2 v_texCoord;

out vec4 fragColor;

#include "shared.frag"

void main( )
{
    vec4 data = texture( u_piercingTexture, v_texCoord );

    float chance = 0.0;
    float damageNormalized = 0.0;
    if (data.a > 0.0 && data.b > 0.0)
    {
        float distance = data.b - data.a;
        float armor = data.r * 768.0;
        if (distance == 0.0)
        {
            float piercingPower = float (u_piercingConstants);
            float effectiveArmor = armor / data.g;
            float armorByMaxPiercing = effectiveArmor / (piercingPower * (1.0 + u_platform.y));
            chance = getPiercingChance( armorByMaxPiercing, u_platform.w, 1.0, u_platform );
        }
        else
            chance = 0.0;

        distance *= u_depthConstants.y;
        if (distance < u_explosionRadius)
        {
            float damageInitial = u_damage;
            float damageHE = u_damage * (1.0 - distance / u_explosionRadius) - armor * 1.3 * u_antifragmentationLiningFactor;
            if (damageHE <= 0.0)
                damageNormalized = 0.0;
            else
                damageNormalized = damageHE / damageInitial;
        }
        else
        {
            damageNormalized = 0.0;
        }
    }

    fragColor.r = chance;
    fragColor.g = damageNormalized;
    fragColor.b = data.a;
    fragColor.a = 0.0;
}