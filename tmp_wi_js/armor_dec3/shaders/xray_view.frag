#version 300 es
#include "lighting_header.frag"

///////////////////////////////////////////////////////////
// Uniforms

#ifndef WHITE
uniform vec2 u_armorGradientRange;
uniform float u_armorConstants;
uniform float u_saturation;
#endif

///////////////////////////////////////////////////////////
// Varyings

#include "shared.frag"

#if defined(CLIP_PLANE)
in float v_clipDistance;
#endif

#if defined(HEATMAP)
#if defined(TANK_CLASSES)
uniform samplerCube u_heatmapCubeArray[5];
uniform vec2 u_heatmapMaxWeightLog;
#else
uniform sampler2D u_heatmapGradient;
uniform samplerCube u_heatmapCube;
#if defined(NO_COLOR)
uniform vec3 u_damagePointPosition;
#else
uniform vec4 u_heatmapColor;
#endif
uniform float u_heatmapFadeout;
#endif
in vec3 v_hitpointDirection;
in vec3 v_hitpointToCameraDirection;
in vec3 v_normalLocal;
in vec3 v_positionVehicleLocal;
#endif

#if defined(HEATMAP) && !defined(TANK_CLASSES)
void sampleHeatmap(samplerCube heatmapSampler, vec3 uvw, vec3 normal, out vec4 heatmap, out float fadeout)
{
    heatmap = texture(heatmapSampler, uvw);

    vec2 dirxy = (heatmap.xy - 0.5) * 2.0;
    vec3 dir = vec3(dirxy, sqrt(max(0.0, 1.0 - dot(dirxy, dirxy))));
    if (dot(normal, dir) < 0.0)
        dir.z = -dir.z;

    fadeout = max(0.0, 1.0 - u_heatmapFadeout * (1.0 - dot(dir, normalize(v_hitpointToCameraDirection))) / heatmap.z);
}
#endif

out vec4 fragColor;

void main()
{
#if defined(CLIP_PLANE)
    if (v_clipDistance < 0.0)
        discard;
#endif

#if defined(WHITE)

    fragColor = vec4(1.0, 1.0, 1.0, 1.0);

#elif defined(HEATMAP)

    vec3 uvw;
    vec3 normal = v_normalLocal;
    vec3 absn = abs(normal);
    if (absn.x > absn.y && absn.x > absn.z)
    {
        uvw = vec3(0.5 - step(normal.x, 0.0), v_hitpointDirection.yz);
    }
    else if (absn.y > absn.z)
    {
        uvw = vec3(v_hitpointDirection.x, 0.5 - step(normal.y, 0.0), v_hitpointDirection.z);
    }
    else
    {
        uvw = vec3(v_hitpointDirection.xy, 0.5 - step(normal.z, 0.0));
    }

#if defined(TANK_CLASSES)

    vec3 classColorsXYZ[5];
    classColorsXYZ[0] = vec3(0.25, 1.25, 0.25);
    classColorsXYZ[1] = vec3(1.25, 1.25, 0.25);
    classColorsXYZ[2] = vec3(1.25, 0.25, 0.25);
    classColorsXYZ[3] = vec3(0.25, 0.25, 1.25);
    classColorsXYZ[4] = vec3(1.25, 0.25, 1.25);

    float totalWeight = 0.0;
    vec3 avgColorXYZ = vec3(0.0);

    vec4 heatmapClass0 = texture(u_heatmapCubeArray[0], uvw);
    float weightClassLinear0 = heatmapClass0.z * 0.996108 + heatmapClass0.w * 0.00389105;
    totalWeight += weightClassLinear0;
    avgColorXYZ += weightClassLinear0 * classColorsXYZ[0];

    vec4 heatmapClass1 = texture(u_heatmapCubeArray[1], uvw);
    float weightClassLinear1 = heatmapClass1.z * 0.996108 + heatmapClass1.w * 0.00389105;
    totalWeight += weightClassLinear1;
    avgColorXYZ += weightClassLinear1 * classColorsXYZ[1];

    vec4 heatmapClass2 = texture(u_heatmapCubeArray[2], uvw);
    float weightClassLinear2 = heatmapClass2.z * 0.996108 + heatmapClass2.w * 0.00389105;
    totalWeight += weightClassLinear2;
    avgColorXYZ += weightClassLinear2 * classColorsXYZ[2];

    vec4 heatmapClass3 = texture(u_heatmapCubeArray[3], uvw);
    float weightClassLinear3 = heatmapClass3.z * 0.996108 + heatmapClass3.w * 0.00389105;
    totalWeight += weightClassLinear3;
    avgColorXYZ += weightClassLinear3 * classColorsXYZ[3];

    vec4 heatmapClass4 = texture(u_heatmapCubeArray[4], uvw);
    float weightClassLinear4 = heatmapClass4.z * 0.996108 + heatmapClass4.w * 0.00389105;
    totalWeight += weightClassLinear4;
    avgColorXYZ += weightClassLinear4 * classColorsXYZ[4];

    if (totalWeight <= 0.0)
      discard;

    avgColorXYZ /= totalWeight;
    totalWeight = log(totalWeight * u_heatmapMaxWeightLog.y + 1.0) * u_heatmapMaxWeightLog.x;

    vec3 color = avgColorXYZ * totalWeight;
    float alpha = totalWeight;

#else
    vec4 heatmap;
    float alpha;

    sampleHeatmap(u_heatmapCube, uvw, normal, heatmap, alpha);
    alpha *= min(1.0, heatmap.a * (1.0 + 10.0 * u_heatmapFadeout));

#if defined(NO_COLOR)
    vec3 color = texture(u_heatmapGradient, vec2(heatmap.a, 1.0)).rgb;

    if (dot(u_damagePointPosition, u_damagePointPosition) > 0.0)
    {
      vec3 positionDelta = u_damagePointPosition.xyz - v_positionVehicleLocal;
      float desaturate = smoothstep(0.3, 0.1, dot(positionDelta, positionDelta));
      color = mix(vec3(grayscale(color)), color, desaturate);
    }
#else
    vec3 color = u_heatmapColor.xyz * mix(u_heatmapColor.w, 1.0, heatmap.a);   // gradient from black to color
#endif

#endif

    #if defined(FIX_ALPHA)
    fragColor = vec4(color * alpha, 1.0 - alpha * 1.4);
    #else
    fragColor = vec4(color * alpha, alpha * 1.4);     // premultiplied alpha
    #endif

#else

#ifdef MODULE
    float fullArmor = (u_armorConstants+0.5) / 16.0;
#else
    float armor = u_armorConstants;
    float fullArmor = armor / u_armorGradientRange.y;
#endif 

    _baseColor = texture(u_diffuseTexture, vec2(fullArmor, 0.5));

    float gray = grayscale(_baseColor.rgb);
    _baseColor.rgb = mix(vec3(gray, gray, gray), _baseColor.rgb, u_saturation);

#ifdef LIGHTING

    _baseColor = srgb2lin(_baseColor);

    vec3 ambientColor = _baseColor.rgb * u_ambientColor;
    vec3 litPixel = getLitPixel();
    vec4 res = lin2srgb(vec4(ambientColor + litPixel, 1.0));
    fragColor = res;

#else

    fragColor = _baseColor;

#endif // WHITE

#endif
}