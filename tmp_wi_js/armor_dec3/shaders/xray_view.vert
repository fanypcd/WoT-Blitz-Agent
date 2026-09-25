#version 300 es

///////////////////////////////////////////////////////////
// Atributes
in vec3 a_position;
in vec2 a_texCoord;
in vec3 a_normal;

///////////////////////////////////////////////////////////
// Uniforms
uniform mat4 u_worldViewProjectionMatrix;

#if defined(HEATMAP) || defined(CLIP_PLANE)
uniform mat4 u_worldMatrix;
#endif

#if defined(CLIP_PLANE)
uniform vec4 u_clipPlane;
out float v_clipDistance;
#endif

#if defined(HEATMAP)
uniform vec3 u_cameraWorldPosition;
uniform mat4 u_worldToHeatmapBoxMatrix;
uniform vec3 u_heatmapComponentCenter;
uniform vec3 u_heatmapComponentInvSize;
uniform mat4 u_worldToVehicleMatrix;

out vec3 v_hitpointDirection;
out vec3 v_hitpointToCameraDirection;
out vec3 v_normalLocal;
out vec3 v_positionVehicleLocal;
#endif

void main()
{
    vec4 position = vec4(a_position, 1.0);
    vec4 positionProjected = u_worldViewProjectionMatrix * position;

    #if defined(CLIP_PLANE) || defined(HEATMAP)
    vec4 worldPoint = u_worldMatrix * position;
    #endif

    #if defined(CLIP_PLANE)
    v_clipDistance = dot(worldPoint, u_clipPlane);
    #endif

#if defined(HEATMAP)
    vec3 hitpointLocal = (u_worldToHeatmapBoxMatrix * worldPoint).xyz;
    v_hitpointDirection = (hitpointLocal - u_heatmapComponentCenter) * u_heatmapComponentInvSize;
    v_hitpointToCameraDirection = (u_worldToHeatmapBoxMatrix * vec4(u_cameraWorldPosition, 1.0)).xyz - hitpointLocal;
    v_normalLocal = (u_worldToHeatmapBoxMatrix * u_worldMatrix * vec4(a_normal, 0.0)).xyz;

    v_positionVehicleLocal = (u_worldToVehicleMatrix * worldPoint).xyz;
#endif

#ifdef WHITE
    positionProjected.z = positionProjected.w;
#endif
    gl_Position = positionProjected;
}