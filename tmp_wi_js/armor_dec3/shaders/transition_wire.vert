#version 300 es

///////////////////////////////////////////////////////////
// Atributes
in vec3 a_position;
in vec3 a_normal;

#if defined(SKINNING)
in vec4 a_blendWeights;
in vec4 a_blendIndices;
#endif

in float a_texCoord1;
out float v_transitionDelta;
uniform float u_transition;
uniform float u_time;

///////////////////////////////////////////////////////////
// Uniforms
uniform mat4 u_worldViewProjectionMatrix;
#if defined(SKINNING)
uniform vec4 u_matrixPalette[SKINNING_JOINT_COUNT * 3];
#endif

#define LIGHTING
#if defined(SKINNING)
#include "skinning.vert"
#else
#include "skinning-none.vert" 
#endif

void main()
{
    vec4 position = getPosition();

    v_transitionDelta = 1.0 - clamp(abs(a_texCoord1 - u_transition) * 15.0, 0.0, 1.0);

    vec3 normal = getNormal();
    position.xyz += normal * v_transitionDelta * (0.09 + 0.02 * sin(u_time * (3.0 + position.x * position.y)));
    
    // get projected position
    vec4 positionProjected = u_worldViewProjectionMatrix * position;
    gl_Position = positionProjected;
}