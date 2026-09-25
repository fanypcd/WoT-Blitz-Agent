#version 300 es
#include "lighting_header.vert"

#if defined(USE_SHADOWS)
uniform mat4 u_worldMatrix;
out vec4 v_worldPoint;
#endif

out vec3 v_screenCoord;

void main()
{
    vec4 position = getPosition();
    vec4 positionProjected = u_worldViewProjectionMatrix * position;
    gl_Position = positionProjected;

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
    
    #if defined(TEXTURE_REPEAT)
    v_texCoord *= u_textureRepeat;
    #endif
    
    #if defined(TEXTURE_OFFSET)
    v_texCoord += u_textureOffset;
    #endif
    
    #if defined(LIGHTMAP)
    v_texCoord1 = a_texCoord1;
    #endif

    v_screenCoord.xy = positionProjected.xy * 0.5 + vec2(0.5, 0.5) * positionProjected.w;
    v_screenCoord.z = positionProjected.w;

#if defined(USE_SHADOWS)
    v_worldPoint = u_worldMatrix * position;
#endif
}