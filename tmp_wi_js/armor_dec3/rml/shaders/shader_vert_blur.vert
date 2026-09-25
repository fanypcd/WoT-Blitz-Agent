#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

#define BLUR_SIZE 7
#define BLUR_NUM_WEIGHTS 4

uniform vec2 _texelOffset;

in vec2 inPosition;
in vec2 inTexCoord0;

out vec2 fragTexCoord[BLUR_SIZE];

void main() {
	for(int i = 0; i < BLUR_SIZE; i++)
		fragTexCoord[i] = inTexCoord0 - float(i - BLUR_NUM_WEIGHTS + 1) * _texelOffset;
    gl_Position = vec4(inPosition, 0.0, 1.0);
}
