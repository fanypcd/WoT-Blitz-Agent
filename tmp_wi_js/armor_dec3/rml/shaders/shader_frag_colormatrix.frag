#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

uniform sampler2D _tex;
uniform mat4 _color_matrix;

in vec2 fragTexCoord;
out vec4 finalColor;

void main() {
	// The general case uses a 4x5 color matrix for full rgba transformation, plus a constant term with the last column.
	// However, we only consider the case of rgb transformations. Thus, we could in principle use a 3x4 matrix, but we
	// keep the alpha row for simplicity.
	// In the general case we should do the matrix transformation in non-premultiplied space. However, without alpha
	// transformations, we can do it directly in premultiplied space to avoid the extra division and multiplication
	// steps. In this space, the constant term needs to be multiplied by the alpha value, instead of unity.
	vec4 texColor = texture(_tex, fragTexCoord);
	vec3 transformedColor = vec3(_color_matrix * texColor);
	finalColor = vec4(transformedColor, texColor.a);
}
