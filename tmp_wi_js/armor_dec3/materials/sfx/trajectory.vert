#version 300 es

in vec3 a_position;

#if defined(CROSS_SECTION)
in vec4 a_color;
out vec4 v_color;
#endif

uniform mat4 u_worldViewProjectionMatrix;

void main()
{
#if defined(CROSS_SECTION)
    v_color = a_color;
#endif
    gl_Position = u_worldViewProjectionMatrix * vec4(a_position, 1.0);
}