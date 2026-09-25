#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

uniform sampler2D _tex;
in vec2 fragTexCoord;
out vec4 finalColor;

void main() {
	finalColor = texture(_tex, fragTexCoord);
}
