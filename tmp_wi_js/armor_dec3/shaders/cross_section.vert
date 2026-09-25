#version 300 es
#ifndef DIRECTIONAL_LIGHT_COUNT
#define DIRECTIONAL_LIGHT_COUNT 0
#endif
#ifndef SPOT_LIGHT_COUNT
#define SPOT_LIGHT_COUNT 0
#endif
#ifndef POINT_LIGHT_COUNT
#define POINT_LIGHT_COUNT 0
#endif
#if (DIRECTIONAL_LIGHT_COUNT > 0) || (POINT_LIGHT_COUNT > 0) || (SPOT_LIGHT_COUNT > 0)
#define LIGHTING
#endif

///////////////////////////////////////////////////////////
// Atributes
in vec4 a_position;

///////////////////////////////////////////////////////////
// Uniforms
uniform mat4 u_worldViewProjectionMatrix;
uniform mat4 u_worldViewMatrix;
uniform vec4 u_clipPlane;

out vec3 v_normalVector;
out vec2 v_texCoord;

#include "lighting.vert"

void main()
{
    vec4 position = a_position;
    vec4 positionProjected = u_worldViewProjectionMatrix * position;

    vec3 u = cross(u_clipPlane.xyz, vec3(u_clipPlane.y, 1.0 - u_clipPlane.y, 0.0));
    vec3 v = cross(u_clipPlane.xyz, u);
    v_texCoord.x = dot(position.xyz, u);
    v_texCoord.y = dot(position.xyz, v);

    vec3 normal = -u_clipPlane.xyz;
    // Transform the normal, tangent and binormals to view space.
    mat3 worldViewMatrix = mat3(u_worldViewMatrix[0].xyz, u_worldViewMatrix[1].xyz, u_worldViewMatrix[2].xyz);
    vec3 normalVector = worldViewMatrix * normal;

    v_normalVector = normalVector;
    applyLight(position);

    gl_Position = positionProjected;
}