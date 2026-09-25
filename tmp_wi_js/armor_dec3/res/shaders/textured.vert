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
// Attributes
in vec3 a_position;

#if defined(SKINNING)
in vec4 a_blendWeights;
in vec4 a_blendIndices;
#endif

in vec2 a_texCoord;

#if defined(LIGHTMAP)
in vec2 a_texCoord1; 
#endif

#if defined(LIGHTING)
in vec3 a_normal;

#if defined(BUMPED)
in vec3 a_tangent;
in vec3 a_binormal;
#endif

#endif

///////////////////////////////////////////////////////////
// Uniforms
uniform mat4 u_worldViewProjectionMatrix;
#if defined(SKINNING)
uniform vec4 u_matrixPalette[SKINNING_JOINT_COUNT * 3];
#endif

#if defined(LIGHTING)
uniform mat4 u_worldViewMatrix;

#if defined(SPECULAR) || (POINT_LIGHT_COUNT > 0) || (SPOT_LIGHT_COUNT > 0)
uniform mat4 u_worldViewMatrix;
#endif

#if defined(BUMPED) && (DIRECTIONAL_LIGHT_COUNT > 0)
uniform vec3 u_directionalLightDirection[DIRECTIONAL_LIGHT_COUNT];
#endif

#if (POINT_LIGHT_COUNT > 0)
uniform vec3 u_pointLightPosition[POINT_LIGHT_COUNT];
#endif

#if (SPOT_LIGHT_COUNT > 0) 
uniform vec3 u_spotLightPosition[SPOT_LIGHT_COUNT];
#if defined(BUMPED)
uniform vec3 u_spotLightDirection[SPOT_LIGHT_COUNT];
#endif
#endif

#if defined(SPECULAR)
uniform vec3 u_cameraPosition;
#endif

#endif

#if defined(TEXTURE_REPEAT)
uniform vec2 u_textureRepeat;
#endif

#if defined(TEXTURE_OFFSET)
uniform vec2 u_textureOffset;
#endif

///////////////////////////////////////////////////////////
// Varyings
out vec2 v_texCoord;

#if defined(LIGHTMAP)
out vec2 v_texCoord1;
#endif

#if defined(LIGHTING)

#if !defined(BUMPED)
out vec3 v_normalVector;
#endif

#if defined(BUMPED) && (DIRECTIONAL_LIGHT_COUNT > 0)
out vec3 v_directionalLightDirection[DIRECTIONAL_LIGHT_COUNT];
#endif

#if (POINT_LIGHT_COUNT > 0)
out vec3 v_vertexToPointLightDirection[POINT_LIGHT_COUNT];
#endif

#if (SPOT_LIGHT_COUNT > 0)
out vec3 v_vertexToSpotLightDirection[SPOT_LIGHT_COUNT];
#if defined(BUMPED)
out vec3 v_spotLightDirection[SPOT_LIGHT_COUNT];
#endif
#endif

#if defined(SPECULAR)
out vec3 v_cameraDirection;
#endif

#include "lighting.vert"

#endif

#if defined(SKINNING)
#include "skinning.vert"
#else
#include "skinning-none.vert" 
#endif

#if defined(CLIP_PLANE) || defined(CLIP_PLANE_INVERSE)
uniform vec4 u_clipPlane;
uniform mat4 u_worldMatrix;
out float v_clipDistance;
#endif

#if defined(GUN_INTERNAL)
out float v_positionZLocal;
#endif

#if defined(TURRET_RING_STRIP)
out float v_normalYLocal;
#endif

void main()
{
    vec4 position = getPosition();
    gl_Position = u_worldViewProjectionMatrix * position;

    #if defined(CLIP_PLANE) || defined(CLIP_PLANE_INVERSE)
    v_clipDistance = dot(u_worldMatrix * position, u_clipPlane);
    #endif

    #if defined(LIGHTING)
    vec3 normal = getNormal();
    // Transform the normal, tangent and binormals to view space.
    mat3 worldViewMatrix = mat3(u_worldViewMatrix[0].xyz, u_worldViewMatrix[1].xyz, u_worldViewMatrix[2].xyz);
    vec3 normalVector = worldViewMatrix * normal;
    
    #if defined(BUMPED)
    
    vec3 tangent = getTangent();
    vec3 binormal = getBinormal();
    vec3 tangentVector  = worldViewMatrix * tangent;
    vec3 binormalVector = worldViewMatrix * binormal;
    mat3 tangentSpaceTransformMatrix = mat3(tangentVector.x, binormalVector.x, normalVector.x, tangentVector.y, binormalVector.y, normalVector.y, tangentVector.z, binormalVector.z, normalVector.z);
    applyLight(position, tangentSpaceTransformMatrix);
    
    #else
    
    v_normalVector = normalVector;
    applyLight(position);
    
    #endif
    
    #endif 
    
    v_texCoord = a_texCoord;
    
    #if defined(TEXTURE_REPEAT)
    v_texCoord *= u_textureRepeat;
    #endif
    
    #if defined(TEXTURE_OFFSET)
    v_texCoord += u_textureOffset;
    #endif
    
    #if defined(LIGHTMAP)
    v_texCoord1 = a_texCoord1;
    #endif

    #if defined(GUN_INTERNAL)
    v_positionZLocal = position.z;
    #endif

    #if defined(TURRET_RING_STRIP)
    v_normalYLocal = a_normal.y;
    #endif
}