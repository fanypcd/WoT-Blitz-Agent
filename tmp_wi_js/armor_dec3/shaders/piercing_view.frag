#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif


///////////////////////////////////////////////////////////
// Uniforms
#if !defined(FIRSTHIT_PASS) && !defined(RICOCHET_RESET)

#ifndef ARMORHQ
uniform float u_piercingConstants;    // piercingPower
uniform vec4 u_platform;
#endif
uniform float u_armorConstants;          // armor

#if defined(USE_HIT_ANGLE) && !defined(RICOCHET_PASS)
uniform vec2 u_normalizationSinCos;    // normalizationAngle (sin, cos)
#endif

#if defined(MAY_RICOCHET) && !defined(ARMORHQ) && !defined(FIRST_SPACED_THICKNESS)
uniform float u_caliber;
uniform float u_ricochetCosAngle;
#endif

#endif


///////////////////////////////////////////////////////////
// Varyings
#if !defined(FIRSTHIT_PASS) && !defined(RICOCHET_RESET) && (defined(USE_HIT_ANGLE) || (defined(MAY_RICOCHET) && !defined(ARMORHQ)))
in vec3 v_normalVector;
in vec3 v_cameraDirection; 
#endif


#if defined(RICOCHET_PASS)
in float v_clipDistance;
#endif

in vec4 v_position;        // xyz - position, w - view distance

out vec4 fragColor;

#include "shared.frag"



void main()
{
#ifdef RICOCHET_PASS
    if (v_clipDistance <= 0.01)
        discard;
#endif

#ifdef RICOCHET_RESET
    fragColor.r = 0.0;
    fragColor.g = 0.0;
    fragColor.b = 0.0;
    fragColor.a = 0.0;
#else

#ifdef FIRSTHIT_PASS
    fragColor.r = 0.0;
    fragColor.g = 0.0;
    fragColor.b = 0.0;
    fragColor.a = v_position.w + hash13(v_position.xyz) / 255.0;
#else

    float armor = u_armorConstants;
#ifndef ARMORHQ
    float piercingPower = u_piercingConstants;
#endif

#if defined(USE_HIT_ANGLE) || (defined(MAY_RICOCHET) && !defined(ARMORHQ))
    vec3 directionVector = normalize(v_cameraDirection);
    vec3 normalVector = normalize(v_normalVector);

    float cosAngle = clamp(abs(dot(directionVector, normalVector)), 0.0001, 1.0);
#endif

    float ricochet = 0.0;

#if defined(MAY_RICOCHET) && !defined(ARMORHQ) && !defined(FIRST_SPACED_THICKNESS)
    if (cosAngle < u_ricochetCosAngle && u_caliber <= 3.0 * armor)
    {
        ricochet = 1.0;
        // ricochets on blitz happen only on very first impact, no need to clear DF=1 thickness since it breaks the pen chance later after clearing ricochet flag
        if (u_platform.x == 0.0)
            armor = 0.0;
    }
#endif

#if defined(USE_HIT_ANGLE) && !defined(RICOCHET_PASS)    // TODO: define __ANDROID__ for android platform
    if (u_normalizationSinCos.x > 0.0)
    {
        // this comparison is buggy on some android devices when doing ricochet pass, so don't use normalization when RICOCHET_PASS is defined
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

#ifdef FIRST_SPACED_THICKNESS
    // effective thickness of frontmost DF=0 plate, same units as accumulated piercing R
    fragColor.r = fullArmor / (piercingPower * (1.0 + u_platform.y));
    fragColor.g = 0.0;
    fragColor.b = 0.0;
    fragColor.a = 0.0;
#else
 
#ifdef ARMORHQ
    fragColor.r = armor / 768.0;
#else
    fragColor.r = fullArmor / (piercingPower * (1.0 + u_platform.y)) + hash13(v_position.xyz) / 255.0;
#endif

#ifdef DF1
    fragColor.b = v_position.w + hash13(v_position.xyz) / 255.0;
    fragColor.g = ricochet * 0.5;
#else
    fragColor.b = 0.0;
    fragColor.g = ricochet;
#endif

#if defined(ARMORHQ) && !defined(ACCUM)
#ifdef USE_HIT_ANGLE
    fragColor.g = cosAngle + hash13(v_position.xyz)/255.0;
#else
    fragColor.g = 1.0;
#endif
#endif

    fragColor.a = 0.0;
#endif // FIRST_SPACED_THICKNESS
#endif // FIRSTHI_PASS

#endif // RICOCHET_RESET
}