#version 300 es
precision mediump float;

uniform sampler2D u_diffuseTexture;
uniform vec2 u_invScreenSize;
in vec2 v_texCoord;
out vec4 fragColor;

void main()
{
    vec4 color;

    color = texture(u_diffuseTexture, v_texCoord + vec2(0.0, -1.407333) * u_invScreenSize) * 0.374310;
    color += texture(u_diffuseTexture, v_texCoord + vec2(0.0, 0.0) * u_invScreenSize) * 0.251379;
    color += texture(u_diffuseTexture, v_texCoord + vec2(0.0, 1.407333) * u_invScreenSize) * 0.374310;

    fragColor.r = texture(u_diffuseTexture, v_texCoord).r;
    fragColor.gba = color.gba;
}