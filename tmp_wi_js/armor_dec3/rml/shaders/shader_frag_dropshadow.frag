#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

uniform sampler2D _tex;
uniform vec2 _texCoordMin;
uniform vec2 _texCoordMax;
uniform vec4 _color;

in vec2 fragTexCoord;
out vec4 finalColor;

void main() {
	vec2 in_region = step(_texCoordMin, fragTexCoord) * step(fragTexCoord, _texCoordMax);
	finalColor = texture(_tex, fragTexCoord).a * in_region.x * in_region.y * _color;
}
