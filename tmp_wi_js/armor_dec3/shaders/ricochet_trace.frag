#version 300 es
precision mediump float;


#if defined(MODULATE_COLOR)
uniform vec4 u_modulateColor;
#endif
uniform float u_alpha;

///////////////////////////////////////////////////////////
// Variables
vec4 _baseColor;

///////////////////////////////////////////////////////////
// Varyings
in vec2 v_texCoord;

out vec4 fragColor;


void main()
{
    _baseColor = vec4(1.0, 1.0, 1.0, u_alpha);

    float uFactor = 1.0;//step(0.5, 1.0 - fract(v_texCoord.x * 10.0));

    #if defined(NO_COLLISION)
    uFactor *= 1.0 - v_texCoord.x;
    #endif

    fragColor.a = _baseColor.a * uFactor;
    fragColor.rgb = _baseColor.rgb;

    #if defined(MODULATE_COLOR)
    fragColor *= u_modulateColor;
    #endif
}