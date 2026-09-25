#version 300 es

///////////////////////////////////////////////////////////
// Atributes
in vec2 a_position;
in vec3 a_texCoord;

///////////////////////////////////////////////////////////
// Uniforms
uniform mat4 u_worldViewProjectionMatrix;

out vec3 v_texCoord;

void main()
{
    float maxDmg = max(a_texCoord.y, a_texCoord.z);
    const float minDistance = 2.0;
    float distance = maxDmg * a_texCoord.x * 0.025;
    
    distance += minDistance;

    vec4 position = vec4(a_position.x * distance, 0.0, a_position.y * distance, 1.0);
    vec4 positionProjected = u_worldViewProjectionMatrix * position;
    gl_Position = positionProjected;

    v_texCoord = a_texCoord;
}