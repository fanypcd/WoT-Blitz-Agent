#version 300 es
///////////////////////////////////////////////////////////
// Attributes
in vec3 a_position;

///////////////////////////////////////////////////////////
// Uniforms
uniform mat4 u_worldViewMatrix;
uniform mat4 u_projectionMatrix;
uniform float u_dispersion;

///////////////////////////////////////////////////////////
// Varyings

out float v_direction;
out float v_transparency;

void main()
{
    v_direction = a_position.z;
    float length = -a_position.z;

    vec3 position = vec3(a_position.xy * u_dispersion * length, length);
    vec3 normal = vec3(a_position.xy, 0.0);

    vec3 viewSpacePos = (u_worldViewMatrix * vec4(position, 1.0)).xyz;
    vec3 viewSpaceNormal = (u_worldViewMatrix * vec4(normal, 0.0)).xyz;
    v_transparency = abs(viewSpaceNormal.z);

    gl_Position = u_projectionMatrix * vec4(viewSpacePos, 1.0);
}