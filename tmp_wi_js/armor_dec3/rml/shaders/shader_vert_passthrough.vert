#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

in vec2 inPosition;
in vec2 inTexCoord0;

out vec2 fragTexCoord;

#ifdef UV_TRANSFORM
uniform vec4 _uvScaleOffset;
#endif

void main() {
#ifdef UV_TRANSFORM
	fragTexCoord = inTexCoord0 * _uvScaleOffset.xy + _uvScaleOffset.zw;
#else
	fragTexCoord = inTexCoord0;
#endif
    gl_Position = vec4(inPosition, 0.0, 1.0);
}
