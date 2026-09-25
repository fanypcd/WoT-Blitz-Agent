#version 300 es
uniform mat4 u_invViewProjectionMatrix;
uniform mat4 u_invWorldMatrix;
uniform float u_groundHeight;
uniform vec3 u_cameraPosition;

in vec2 a_position;

out vec3 v_toGround;    // vehicle local
out vec3 v_cameraPositionVehicleLocal;

void main()
{
    gl_Position = vec4(a_position.x * 2.0 - 1.0, a_position.y * u_groundHeight - 1.0, 0.0, 1);
    vec4 worldPos = u_invViewProjectionMatrix * gl_Position;

    // intersection with XZ
    v_toGround = (u_invWorldMatrix * vec4(worldPos.xyz / worldPos.w - u_cameraPosition, 0)).xyz;
    v_cameraPositionVehicleLocal = (u_invWorldMatrix * vec4(u_cameraPosition, 1.0)).xyz;
}