#version 300 es

///////////////////////////////////////////////////////////
// Atributes
in vec3 a_position;
in vec2 a_texCoord;
in vec3 a_normal;

///////////////////////////////////////////////////////////
// Uniforms
uniform mat4 u_worldViewProjectionMatrix;

#if defined(USE_HIT_ANGLE)
uniform mat4 u_worldViewMatrix;
uniform vec3 u_cameraPosition;
#endif

///////////////////////////////////////////////////////////
// Varyings
#if defined(USE_HIT_ANGLE)
out vec3 v_normalVector;
out vec3 v_cameraDirection;
#endif



void main()
{
    vec4 position = vec4(a_position, 1.0);
    vec4 positionProjected = u_worldViewProjectionMatrix * position;
    gl_Position = positionProjected;

#if defined(USE_HIT_ANGLE)
    vec3 normal = a_normal;
    mat3 worldViewMatrix = mat3(u_worldViewMatrix[0].xyz, u_worldViewMatrix[1].xyz, u_worldViewMatrix[2].xyz);
    v_normalVector = worldViewMatrix * normal;
    
    vec4 positionWorldViewSpace = u_worldViewMatrix * position;
    v_cameraDirection = u_cameraPosition - positionWorldViewSpace.xyz;
#endif
}