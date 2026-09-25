#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

uniform vec3 u_lightDirection;
in vec3 v_worldPos;
in vec3 v_rayleigh;
in vec3 v_mie;

uniform vec3 u_horizonColor;
uniform float u_cloudyFactor;

out vec4 fragColor;

#include "shared.frag"

void main()
{
    const float g = 0.45;
    const float g2 = g * g;
    float s = 0.999 - 0.008 * u_cloudyFactor;
    float s2 = s;
    const float I = 14.0;
    const float SI = 30.0;

    vec3 pos = normalize(v_worldPos);
    vec3 fsun = -u_lightDirection;
    vec3 color;

    float mu = dot(pos, fsun);
    float opmu2 = 1. + mu*mu;
    //float fMiePhase = 1.5 * ((1.0 - g2) / (2.0 + g2)) * (1.0 + mu*mu) / pow(1.0 + g2 - 2.0*g*mu, 1.5);
    float phaseR = 1.0;//0.75 * opmu2;
    float phaseM = 1.5 * (1. - g2) * opmu2 / ((2. + g2) * pow(1. + g2 - 2.*g*mu, 1.5));
    float phaseS = 1.5 * (1. - s2) * opmu2 / ((2. + s2) * pow(1. + s2 - 2.*s*mu, 1.5));
    color = phaseR * v_rayleigh * I + (phaseM * I + phaseS * SI) * v_mie;

    //color = 1.0 - exp(color * -2.0);
    //color = lin2srgb(color);
    //color = pow(color, vec3(1.0/1.2));

    color = mix(color, vec3(grayscale(color)), u_cloudyFactor * 0.8);
    color = mix(u_horizonColor, color, smoothstep(0.0, 0.07 - (0.5 + 0.5 * pos.z) * 0.06, pos.y));

    fragColor.rgb = color + hash33(pos)/255.0;
    fragColor.a = 1.0;
}