#version 300 es

///////////////////////////////////////////////////////////
// Atributes
in vec3 a_position;
in vec2 a_texCoord;

///////////////////////////////////////////////////////////
// Uniforms
uniform mat4 u_worldViewProjectionMatrix;

///////////////////////////////////////////////////////////
// Varyings

void main()
{
    vec4 positionProjected = u_worldViewProjectionMatrix * vec4(a_position, 1.0);
    positionProjected.z = 0.0;
    gl_Position = positionProjected;
}