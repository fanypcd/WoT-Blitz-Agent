#version 300 es
///////////////////////////////////////////////////////////
// Attributes
in vec3 a_position;
in vec4 a_texCoord0; // direction - width
in vec4 a_texCoord1; // data

///////////////////////////////////////////////////////////
// Uniforms
uniform mat4 u_worldViewMatrix;
uniform mat4 u_projectionMatrix;

///////////////////////////////////////////////////////////
// Varyings

out vec4 v_data;

void main()
{
    vec3 toCamera = (u_worldViewMatrix * vec4(a_position, 1.0)).xyz;
    vec3 direction = (u_worldViewMatrix * vec4(a_texCoord0.xyz, 0.0)).xyz;
    vec3 v = normalize(cross(direction, toCamera));

    v_data = a_texCoord1;

    // Transform position to clip space.
    vec3 worldPos = toCamera + v * a_texCoord0.w;
    gl_Position = u_projectionMatrix * vec4(worldPos, 1.0);
}