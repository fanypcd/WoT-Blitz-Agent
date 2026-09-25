#version 300 es
precision mediump float;

uniform sampler2D u_penetrationRamp;

///////////////////////////////////////////////////////////
// Variables

///////////////////////////////////////////////////////////
// Varyings
in vec4 v_penPower;

out vec4 fragColor;

void main()
{
    float t = clamp(max(v_penPower.x, v_penPower.y), 0.0, 1.0);
    if (t == 0.0)
        discard;

    fragColor.rgb = texture(u_penetrationRamp, vec2(t, 0.5)).rgb;
    fragColor.a = v_penPower.w;
}