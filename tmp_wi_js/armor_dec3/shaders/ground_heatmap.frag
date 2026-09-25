#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

#if defined(UI_RENDER)
in vec2 fragTexCoord;
in vec4 fragColor;
uniform vec4 u_dotColor;
#else
in vec3 v_toGround;
in vec3 v_cameraPositionVehicleLocal;
#endif

uniform vec3 u_trajectoryPosition;
uniform float u_maxDamageDistance;
uniform vec3 u_vehicleAngles;

uniform sampler2D u_classHeatmaps[5];
uniform vec2 u_maxWeightInvLog;

#if defined(IMPACTS)
uniform sampler2D u_heatmapGradient;
#endif

uniform vec3 u_damagePoint;

out vec4 _fragColor;

#include "shared.frag"


void main()
{
#if defined(UI_RENDER)
    vec2 uv = 2.0 * (fragTexCoord - vec2(0.5));
    vec2 worldPosition = vec2(uv.x, -uv.y) * u_maxDamageDistance;
#else
    float t = v_cameraPositionVehicleLocal.y / -v_toGround.y;
    vec2 worldPosition = vec2(v_cameraPositionVehicleLocal.x + v_toGround.x * t, v_cameraPositionVehicleLocal.z + v_toGround.z * t);
#endif

    float theta = atan(worldPosition.x, worldPosition.y);
    theta = 3.141592 + theta;

    float distance = length(worldPosition);
    if (distance > u_maxDamageDistance || distance < 5.0)
        discard;

    float hullU = theta / 6.283184;//log(theta * 0.1 + 1.0) / log(3.141592 * 0.1 + 1.0);
    float turretU = mod(theta - u_vehicleAngles.y, 6.283184) / 6.283184;//log(theta * 0.1 + 1.0) / log(3.141592 * 0.1 + 1.0);
    float texV = (sqrt(distance * 0.2) - 1.0) / (sqrt(u_maxDamageDistance * 0.2) - 1.0); // dense    

#if defined(IMPACTS)

    vec2 weightsHull = texture(u_classHeatmaps[0], vec2(hullU, texV)).rg;
    vec2 weightsTurret = texture(u_classHeatmaps[0], vec2(turretU, texV)).ba;
    float totalWeight = (weightsHull.r + weightsTurret.r) * 0.996108 + (weightsHull.g + weightsTurret.g) * 0.00389105;  // 255 * 256 / 65535 and 255 / 65535

    if (totalWeight > 0.0)
      totalWeight = log(totalWeight * u_maxWeightInvLog.y + 1.0) * u_maxWeightInvLog.x;

    vec3 avgColorXYZ = texture(u_heatmapGradient, vec2(totalWeight, 0)).rgb;

#else
    vec3 classColorsXYZ[5];
    classColorsXYZ[0] = vec3(0.25, 1.25, 0.25);
    classColorsXYZ[1] = vec3(1.25, 1.25, 0.25);
    classColorsXYZ[2] = vec3(1.25, 0.25, 0.25);
    classColorsXYZ[3] = vec3(0.25, 0.25, 1.25);
    classColorsXYZ[4] = vec3(1.25, 0.25, 1.25);

    vec3 avgColorXYZ = vec3(0.0);
    float totalWeight = 0.0;

    // Manually unroll for each vehicle type
    vec2 weightsHull0 = texture(u_classHeatmaps[0], vec2(hullU, texV)).rg;
    vec2 weightsTurret0 = texture(u_classHeatmaps[0], vec2(turretU, texV)).ba;
    float weightClassLinear0 = (weightsHull0.r + weightsTurret0.r) * 0.996108 + (weightsHull0.g + weightsTurret0.g) * 0.00389105;
    totalWeight += weightClassLinear0;
    avgColorXYZ += classColorsXYZ[0] * weightClassLinear0;

    vec2 weightsHull1 = texture(u_classHeatmaps[1], vec2(hullU, texV)).rg;
    vec2 weightsTurret1 = texture(u_classHeatmaps[1], vec2(turretU, texV)).ba;
    float weightClassLinear1 = (weightsHull1.r + weightsTurret1.r) * 0.996108 + (weightsHull1.g + weightsTurret1.g) * 0.00389105;
    totalWeight += weightClassLinear1;
    avgColorXYZ += classColorsXYZ[1] * weightClassLinear1;

    vec2 weightsHull2 = texture(u_classHeatmaps[2], vec2(hullU, texV)).rg;
    vec2 weightsTurret2 = texture(u_classHeatmaps[2], vec2(turretU, texV)).ba;
    float weightClassLinear2 = (weightsHull2.r + weightsTurret2.r) * 0.996108 + (weightsHull2.g + weightsTurret2.g) * 0.00389105;
    totalWeight += weightClassLinear2;
    avgColorXYZ += classColorsXYZ[2] * weightClassLinear2;

    vec2 weightsHull3 = texture(u_classHeatmaps[3], vec2(hullU, texV)).rg;
    vec2 weightsTurret3 = texture(u_classHeatmaps[3], vec2(turretU, texV)).ba;
    float weightClassLinear3 = (weightsHull3.r + weightsTurret3.r) * 0.996108 + (weightsHull3.g + weightsTurret3.g) * 0.00389105;
    totalWeight += weightClassLinear3;
    avgColorXYZ += classColorsXYZ[3] * weightClassLinear3;

    vec2 weightsHull4 = texture(u_classHeatmaps[4], vec2(hullU, texV)).rg;
    vec2 weightsTurret4 = texture(u_classHeatmaps[4], vec2(turretU, texV)).ba;
    float weightClassLinear4 = (weightsHull4.r + weightsTurret4.r) * 0.996108 + (weightsHull4.g + weightsTurret4.g) * 0.00389105;
    totalWeight += weightClassLinear4;
    avgColorXYZ += classColorsXYZ[4] * weightClassLinear4;

    if (totalWeight <= 0.0)
      discard;

    avgColorXYZ /= max(0.0001, totalWeight);
    totalWeight = log(totalWeight * u_maxWeightInvLog.y + 1.0) * u_maxWeightInvLog.x;
    avgColorXYZ *= totalWeight;   // gradient to black
#endif

    vec2 trajectory2d = vec2(u_trajectoryPosition.x, u_trajectoryPosition.z);

    // damage point on tank is set
    if (u_damagePoint.y > 0.0)
    {
      const float distanceSigma2 = 10000.0;
      float dDistance = u_damagePoint.y - distance;
      float w_distance = exp(-dDistance * dDistance / distanceSigma2);

      float varianceSigma2 = u_damagePoint.z;
      float dDirection = abs(u_damagePoint.x - (theta - 3.141592));
      dDirection = min(dDirection, 6.143184 - dDirection);
      float w_theta = exp(-dDirection * dDirection / varianceSigma2);

      avgColorXYZ = mix(vec3(grayscale(avgColorXYZ)), avgColorXYZ, w_distance * w_theta);

#if defined(UI_RENDER)
      vec2 highlight = vec2(sin(u_damagePoint.x) * u_damagePoint.y, cos(u_damagePoint.x) * u_damagePoint.y) - worldPosition;
      float poleLengthSq = dot(highlight, highlight);
      if (poleLengthSq < 0.001 * u_maxDamageDistance * u_maxDamageDistance)
      {
        avgColorXYZ = u_dotColor.rgb;
      }
#endif
    }
    else
    {
#if defined(UI_RENDER)
      // draw dot
      vec2 dotpos = trajectory2d - worldPosition;
      if (dot(dotpos, dotpos) < 0.001 * u_maxDamageDistance * u_maxDamageDistance)
      {
        avgColorXYZ = u_dotColor.rgb;
      }
#else
      // tooltip is visible - ground hit
      if (dot(trajectory2d, trajectory2d) > 0.0)
      {
        vec2 highlight = trajectory2d - worldPosition;
        float poleLengthSq = dot(highlight, highlight);
        if (poleLengthSq > 9.0)
          avgColorXYZ = vec3(grayscale(avgColorXYZ));
      }
#endif
    }

    _fragColor.rgb = avgColorXYZ;

#if defined(UI_RENDER)
    _fragColor.a = 1.0;
    _fragColor *= fragColor;
#else
    _fragColor.a = totalWeight;
#endif
}