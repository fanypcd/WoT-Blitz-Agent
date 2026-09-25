#version 300 es

///////////////////////////////////////////////////////////
// Atributes
in vec3 a_position;
in vec2 a_texCoord;
in vec3 a_normal;

///////////////////////////////////////////////////////////
// Uniforms
uniform mat4 u_worldViewProjectionMatrix;

#if !defined(FIRSTHIT_PASS) && !defined(RICOCHET_RESET) 

#if defined(USE_HIT_ANGLE) || (defined(MAY_RICOCHET) && !defined(ARMORHQ))
#if !defined(DF1) && !defined(RICOCHET_PASS)
uniform mat4 u_worldViewMatrix;
#endif
uniform vec3 u_cameraPosition;
#endif

///////////////////////////////////////////////////////////
// Varyings
#if defined(USE_HIT_ANGLE) || (defined(MAY_RICOCHET) && !defined(ARMORHQ))
out vec3 v_normalVector;
out vec3 v_cameraDirection;
#endif

#endif


#if defined(DF1) || defined(FIRSTHIT_PASS) || defined(RICOCHET_PASS)
uniform mat4 u_worldViewMatrix;
#endif

#if defined(DF1) || defined(FIRSTHIT_PASS)
uniform vec2 u_depthConstants;
#endif

#if defined(RICOCHET_PASS)
uniform vec4 u_clipPlaneReflectedView;
out float v_clipDistance;
#endif

out vec4 v_position;        // xyz - position, w - view distance


void main()
{
    vec4 position = vec4(a_position, 1.0);
    vec4 positionProjected = u_worldViewProjectionMatrix * position;
    gl_Position = positionProjected;

#if !defined(FIRSTHIT_PASS) && !defined(RICOCHET_RESET) 
#if defined(USE_HIT_ANGLE) || (defined(MAY_RICOCHET) && !defined(ARMORHQ))
    vec3 normal = a_normal;
    mat3 worldViewMatrix = mat3(u_worldViewMatrix[0].xyz, u_worldViewMatrix[1].xyz, u_worldViewMatrix[2].xyz);
    v_normalVector = worldViewMatrix * normal;
    
    vec4 positionWorldViewSpace = u_worldViewMatrix * position;
    v_cameraDirection = u_cameraPosition - positionWorldViewSpace.xyz;
#endif
#endif

#if defined(DF1) || defined(FIRSTHIT_PASS) || defined(RICOCHET_PASS)
    vec4 pos = u_worldViewMatrix * position;
#endif

#if defined(DF1) || defined(FIRSTHIT_PASS)
    v_position.w = (-pos.z - u_depthConstants.x) / u_depthConstants.y;
#else
    v_position.w = 0.0;
#endif

#ifdef RICOCHET_PASS
    v_clipDistance = dot(pos, u_clipPlaneReflectedView);
#endif

    v_position.xyz = a_position.xyz;
}