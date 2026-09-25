#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

uniform vec3 u_lightDirection;
in vec3 v_toAtmosphere;
in vec3 v_rayleigh;
in vec3 v_mie;

uniform vec3 u_horizonColor;

uniform sampler2D u_shadowMap;
uniform mat4 u_shadowMapViewProjection0;

uniform vec3 u_ambientColor;
uniform vec3 u_directionalLightColor[1];
//uniform float u_cloudyFactor;

uniform vec3 u_cameraPosition;

out vec4 fragColor;

#include "shared.frag"

void main()
{    
    const float g = 0.45;
    const float g2 = g * g;
    const float s = 0.99;
    const float s2 = s;
    const float I = 14.0;
    const float SI = 30.0;

    vec3 pos = normalize(v_toAtmosphere);
    vec3 fsun = -u_lightDirection;
    vec3 color;

    float mu = dot(pos, fsun);
    float opmu2 = 1. + mu*mu;
    //float fMiePhase = 1.5 * ((1.0 - g2) / (2.0 + g2)) * (1.0 + mu*mu) / pow(1.0 + g2 - 2.0*g*mu, 1.5);
    float phaseR = 1.0;//0.75 * opmu2;
    float phaseM = 1.5 * (1. - g2) * opmu2 / ((2. + g2) * (1. + g2 - 2.*g*mu));
    float phaseS = 1.5 * (1. - s2) * opmu2 / ((2. + s2) * (1. + s2 - 2.*s*mu));
    color = phaseR * v_rayleigh * I + (phaseM * I + phaseS * SI) * v_mie;

    //color = 1.0 - exp(color * -2.0);
    //color = lin2srgb(color);
    //color = pow(color, vec3(1.0/1.2));

    //vec3 irradianceContribution = texture(u_skySpecularTexture, vec2(1.0, (1.0 - dot(N, v_upDirection)) * 0.5)).rgb * u_ambientColor;

    float shadowFactor = fsun.y; // NdotL

    // found intersection with y=0 plane
    float t = u_cameraPosition.y / v_toAtmosphere.y;
    float worldPosition_x = u_cameraPosition.x + v_toAtmosphere.x * t;
    float worldPosition_z = u_cameraPosition.z + v_toAtmosphere.z * t;

    vec4 shadowMapPlanePoint = u_shadowMapViewProjection0 * vec4(worldPosition_x, 0.0, worldPosition_z, 1.0);
    vec2 shadowMapUV = shadowMapPlanePoint.xy / shadowMapPlanePoint.w * 0.5 + vec2(0.5, 0.5);
    if (shadowMapUV.x > 0.0 && shadowMapUV.x < 1.0 && shadowMapUV.y > 0.0 && shadowMapUV.y < 1.0)
    {
        vec4 depthSample = texture(u_shadowMap, shadowMapUV);
        if (depthSample.r * depthSample.g * depthSample.b * depthSample.a < 1.0)    // check that depth buffer has values other than 1.0
            shadowFactor *= 0.25;
    }

    vec3 albedo = vec3(0.5787, 0.5551, 0.4656);  // in linear space

    float rSq = sqrt(worldPosition_x * worldPosition_x + worldPosition_z * worldPosition_z);
    float outsideCircle = smoothstep(3.0, 10.0, rSq);
    albedo *= 1.0 - 0.2 * outsideCircle;

    //color = mix(color, vec3(grayscale(color)), u_cloudyFactor * 0.8);

    vec3 diffuseContribution = u_directionalLightColor[0] * 0.7 * shadowFactor;
    vec3 irradianceContribution = (color + u_ambientColor) * 0.5;

    color = albedo * (diffuseContribution + irradianceContribution);
    color = mix(u_horizonColor, color, smoothstep(0.0, 0.04 - (0.5 + 0.5 * pos.z) * 0.03, pos.y)) + hash33(pos)/255.0;

    fragColor.rgb = color;
    fragColor.a = 1.0;
}