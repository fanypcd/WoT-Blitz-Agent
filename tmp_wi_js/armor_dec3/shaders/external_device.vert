#version 300 es

///////////////////////////////////////////////////////////
// Atributes
in vec3 a_position;
in vec2 a_texCoord;
in vec3 a_normal;

///////////////////////////////////////////////////////////
// Uniforms
uniform mat4 u_worldViewProjectionMatrix;

void main()
{
    gl_Position = u_worldViewProjectionMatrix * vec4(a_position, 1.0);
}