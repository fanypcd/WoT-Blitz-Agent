#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif


///////////////////////////////////////////////////////////
// Uniforms
uniform float u_piercingConstants;    // piercingPower
uniform float u_armorConstants;       // armor
#if defined(DF1)
uniform float u_damage;
#endif
uniform vec4 u_platform;

#if defined(USE_HIT_ANGLE)
uniform vec2 u_normalizationSinCos;    // normalizationAngle (sin, cos)
#endif


///////////////////////////////////////////////////////////
// Varyings
#if defined(USE_HIT_ANGLE)
in vec3 v_normalVector;
in vec3 v_cameraDirection; 
#endif

out vec4 fragColor;

#include "shared.frag"



void main()
{
    float armor = u_armorConstants;
    float piercingPower = u_piercingConstants;

#if defined(USE_HIT_ANGLE)
    vec3 directionVector = normalize(v_cameraDirection);
    vec3 normalVector = normalize(v_normalVector);

    float cosAngle = clamp(abs(dot(directionVector, normalVector)), 0.0001, 1.0);
#endif

    float ricochet = 0.0;

#if defined(USE_HIT_ANGLE)
    if (u_normalizationSinCos.x > 0.0)
    {
        if (cosAngle > u_normalizationSinCos.y)
        {
            cosAngle = 1.0;
        }
        else
        {
            float sinAngle = sqrt( 1.0 - cosAngle * cosAngle );
            cosAngle = cosAngle * u_normalizationSinCos.y + sinAngle * u_normalizationSinCos.x;
        }
    }
#endif

    float fullArmor = armor;
#ifdef USE_HIT_ANGLE
    fullArmor /= cosAngle;
#endif

    fullArmor /= piercingPower * (1.0 + u_platform.y);

#if defined(FIRSTHIT_PASS)

    fragColor.r = 0.0;
    fragColor.g = 0.0;
    fragColor.b = 0.0;
    fragColor.a = fullArmor;

#else


#ifdef DF1
    fragColor.r = fullArmor;
    fragColor.g = armor > u_damage * 0.1 ? u_damage * 0.1 / armor : 1.0;
    fragColor.b = 0.0;
    fragColor.a = 0.0;
#else
    fragColor.r = 3.0 * fullArmor;
    fragColor.g = 0.0;
    fragColor.b = fullArmor;
    fragColor.a = 0.0;
#endif

#endif
}