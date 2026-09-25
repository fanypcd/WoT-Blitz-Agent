#version 300 es

///////////////////////////////////////////////////////////
// Atributes
in vec3 a_position;
in vec2 a_texCoord;


///////////////////////////////////////////////////////////
// Uniforms
uniform mat4 u_worldViewProjectionMatrix;


#if defined(CLIP_PLANE)
uniform mat4 u_worldMatrix;
uniform vec4 u_clipPlane;
out float v_clipDistance;
#endif

#if defined(CLIP_INTERIOR) || defined(ALPHA_TEST)
out vec2 v_texCoord;
#endif


void main()
{
    vec4 position = vec4(a_position, 1.0);

    #if defined(CLIP_PLANE)
    v_clipDistance = dot(u_worldMatrix * position, u_clipPlane);
    #endif

    #if defined(CLIP_INTERIOR) || defined(ALPHA_TEST)
    v_texCoord = a_texCoord;
    #endif

    gl_Position = u_worldViewProjectionMatrix * position;
}