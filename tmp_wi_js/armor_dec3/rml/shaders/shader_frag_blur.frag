#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

#define BLUR_SIZE 7
#define BLUR_NUM_WEIGHTS 4

uniform sampler2D _tex;
uniform float _weights[BLUR_NUM_WEIGHTS];
uniform vec2 _texCoordMin;
uniform vec2 _texCoordMax;

in vec2 fragTexCoord[BLUR_SIZE];
out vec4 finalColor;

void main() {
	vec4 color = vec4(0.0, 0.0, 0.0, 0.0);
	for(int i = 0; i < BLUR_SIZE; i++)
	{
		vec2 in_region = step(_texCoordMin, fragTexCoord[i]) * step(fragTexCoord[i], _texCoordMax);
		color += texture(_tex, fragTexCoord[i]) * in_region.x * in_region.y * _weights[abs(i - BLUR_NUM_WEIGHTS + 1)];
	}
	finalColor = color;
}
