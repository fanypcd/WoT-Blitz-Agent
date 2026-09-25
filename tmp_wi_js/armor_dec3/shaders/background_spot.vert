#version 300 es
///////////////////////////////////////////////////////////
// Atributes
in vec4 a_position;
in vec2 a_texCoord;

///////////////////////////////////////////////////////////
// Uniforms
uniform mat4 u_worldViewProjectionMatrix;
uniform float u_scale;

out vec4 v_worldPosition;
out vec2 v_texCoord;

#ifdef HEDAMAGE
out vec3 v_screenCoord;
#endif

#ifdef RAMMING
uniform mat4 u_worldToVehicleMatrix;
out vec4 v_groundVehicleLocalPosition;
#endif

void main()
{
    vec4 position = vec4(a_position.xyz * u_scale, 1.0);
    vec4 positionProjected = u_worldViewProjectionMatrix * position;
    gl_Position = positionProjected;
    v_worldPosition = position;
    v_texCoord = a_texCoord;

#ifdef HEDAMAGE
    v_screenCoord.xy = positionProjected.xy * 0.5 + vec2(0.5, 0.5) * positionProjected.w;
    v_screenCoord.z = positionProjected.w;
#endif

#ifdef RAMMING
    v_groundVehicleLocalPosition = u_worldToVehicleMatrix * v_worldPosition;
#endif
}