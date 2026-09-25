#version 300 es

///////////////////////////////////////////////////////////
// Atributes
in vec2 a_position;
in vec2 a_texCoord;

///////////////////////////////////////////////////////////
// Uniforms

uniform mat4 u_projectionMatrix;

out vec2 v_texCoord;


void main()
{
    vec4 position = u_projectionMatrix * vec4(a_position.xy, 0.0, 1.0);
    gl_Position = position;
    v_texCoord = a_texCoord;
}