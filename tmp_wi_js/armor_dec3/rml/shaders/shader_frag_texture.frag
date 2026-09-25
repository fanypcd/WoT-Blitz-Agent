#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

uniform sampler2D _tex;
in vec2 fragTexCoord;
in vec4 fragColor;

#ifdef ANIMATED
uniform float _time;
uniform float _fps;
uniform vec2 _frameSize;
#endif

out vec4 finalColor;

void main() {
#ifdef ANIMATED
    vec2 uv = vec2(fragTexCoord.x * _frameSize.x, fragTexCoord.y * _frameSize.y);
    uv.x += mod(floor(_time * _fps), 1.0 / _frameSize.x) * _frameSize.x;
    vec4 texColor = texture(_tex, uv);
#else
	vec4 texColor = texture(_tex, fragTexCoord);
#endif
	finalColor = fragColor * texColor;
}
