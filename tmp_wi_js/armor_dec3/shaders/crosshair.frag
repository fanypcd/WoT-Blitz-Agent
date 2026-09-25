#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

in vec2 v_texCoord;
in vec4 v_color;

uniform float u_crosshairSize; // crosshair size in pixels

out vec4 fragColor;

void main()
{
    float emptySpace = 4.0 / 32.0;

    // make width of circle no more than 2px
    float width = 1.0 / u_crosshairSize;

    vec2 uv = abs(v_texCoord.xy - vec2(0.5, 0.5));

    // render center dot
    if (uv.x * uv.x + uv.y * uv.y > width * width)
    {
        // skip space at center
        if (uv.x < emptySpace && uv.y < emptySpace)
            discard;

        // skip other empty space
        if (uv.x > emptySpace && uv.y > width)
            discard;

        if (uv.y > emptySpace && uv.x > width)
            discard;
    }

    fragColor.rgb = v_color.rgb;
    fragColor.a = 1.0;
}