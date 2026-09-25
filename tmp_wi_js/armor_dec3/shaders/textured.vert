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

#if defined(VERSION_HD) && defined(TRANSITION_FX)
in float a_texCoord1;
out float v_transitionDelta;
uniform float u_transition;
uniform float u_time;
#endif

///////////////////////////////////////////////////////////
// Uniforms
uniform mat4 u_worldViewProjectionMatrix;
#if defined(SKINNING)
uniform vec4 u_matrixPalette[SKINNING_JOINT_COUNT * 3];
#endif

#if defined(LIGHTING)
uniform mat4 u_worldViewMatrix;

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

uniform mat4 u_viewMatrix;

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

#if defined(TRACKS)
uniform float u_tracksRotation;
#endif

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

out vec3 v_upDirection; // direction of world's up vector in lighting space, either view space or tangent space (for bumped surfaces)

#include "lighting.vert"

#endif

#if defined(SKINNING)
#include "skinning.vert"
#else
#include "skinning-none.vert" 
#endif

#if defined(USE_SHADOWS) || defined(CLIP_PLANE)
uniform mat4 u_worldMatrix;
#endif

#if defined(USE_SHADOWS)
out vec4 v_worldPoint;
#endif

#if defined(HITSKIN)
out vec3 v_screenCoord;
#endif

#if defined(CLIP_PLANE)
uniform vec4 u_clipPlane;
out float v_clipDistance;
#endif


void main()
{
    vec4 position = getPosition();

#if defined(VERSION_HD) && defined(TRANSITION_FX)
    v_transitionDelta = clamp(abs(a_texCoord1 - u_transition) * 15.0, 0.0, 1.0);
#endif

    // lighting
    #if defined(LIGHTING)
    vec3 normal = getNormal();
    // Transform the normal, tangent and binormals to view space.
    mat3 worldViewMatrix = mat3(u_worldViewMatrix[0].xyz, u_worldViewMatrix[1].xyz, u_worldViewMatrix[2].xyz);
    vec3 normalVector = worldViewMatrix * normal;

    // alter position for trasition fx
#if defined(VERSION_HD) && defined(TRANSITION_FX)
    position.xyz += normal * (1.0 - v_transitionDelta) * (0.15 + 0.05 * sin(u_time * (3.0 + position.x * position.y)));
#endif
    
    #if defined(BUMPED)
    
    vec3 tangent = getTangent();
    vec3 binormal = getBinormal();
    vec3 tangentVector  = worldViewMatrix * tangent;
    vec3 binormalVector = worldViewMatrix * binormal;
    mat3 tangentSpaceTransformMatrix = mat3(tangentVector.x, binormalVector.x, normalVector.x, tangentVector.y, binormalVector.y, normalVector.y, tangentVector.z, binormalVector.z, normalVector.z);
    applyLight(position, tangentSpaceTransformMatrix);

    v_upDirection = tangentSpaceTransformMatrix * u_viewMatrix[1].xyz;
    
    #else
    
    v_normalVector = normalVector;
    applyLight(position);
    v_upDirection = u_viewMatrix[1].xyz;
    
    #endif
    
    #endif 


    // get projected position

    vec4 positionProjected = u_worldViewProjectionMatrix * position;
    gl_Position = positionProjected;

    // other attributes

    #if defined(CLIP_PLANE)
    v_clipDistance = dot(u_worldMatrix * position, u_clipPlane);
    #endif
    
    v_texCoord = a_texCoord;

    #if defined(TRACKS)
    v_texCoord.y += u_tracksRotation * sign(position.x);
    #endif
    
    #if defined(TEXTURE_REPEAT)
    v_texCoord *= u_textureRepeat;
    #endif
    
    #if defined(TEXTURE_OFFSET)
    v_texCoord += u_textureOffset;
    #endif
    
    #if defined(LIGHTMAP)
    v_texCoord1 = a_texCoord1;
    #endif

#if defined(HITSKIN)
    v_screenCoord.xy = positionProjected.xy * 0.5 + vec2(0.5, 0.5) * positionProjected.w;
    v_screenCoord.z = positionProjected.w;
#endif

#if defined(USE_SHADOWS)
    vec4 worldPoint = u_worldMatrix * position;
#endif

#if defined(USE_SHADOWS)
    v_worldPoint = worldPoint;
#endif
}