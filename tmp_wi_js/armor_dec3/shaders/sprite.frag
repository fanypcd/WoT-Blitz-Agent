#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

in vec2 v_texCoord;
uniform vec4 u_color;
uniform sampler2D u_texture;

out vec4 fragColor;

void main()
{
    fragColor = u_color * texture(u_texture, v_texCoord);
}