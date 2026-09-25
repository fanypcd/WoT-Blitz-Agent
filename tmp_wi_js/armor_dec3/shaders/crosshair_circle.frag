#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

in vec2 v_texCoord;
in vec4 v_color;

uniform float u_circleSize; // circle size in pixels

out vec4 fragColor;

void main()
{
    const float radius = 0.5;

    // make width of circle no more than 2px
    float width = 1.0 / u_circleSize;

    vec2 uv = v_texCoord.xy - vec2(0.5, 0.5);
    float rSq = dot(uv, uv);
    float rMaxSq = radius * radius;
    float rMinSq = radius - 2.0 * width;
    rMinSq *= rMinSq;

    if (rSq < rMinSq)
        discard;
    if (rSq > rMaxSq)
        discard;

    float alpha = (rSq - rMinSq) / (rMaxSq - rMinSq);
    alpha = 1.0 - abs(0.5 - alpha) * 2.0;

    float phi = atan(uv.y, uv.x);
    alpha *= min(0.75, cos(30.0 * phi) * 5.0);

    if (alpha <= 0.0)
        discard;

    fragColor.rgb = v_color.rgb;
    fragColor.a = 1.0;
}