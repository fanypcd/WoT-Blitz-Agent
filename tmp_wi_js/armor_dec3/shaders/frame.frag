#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

in vec2 v_texCoord;
in vec4 v_color;

uniform vec2 u_size; // width/height in pixels

out vec4 fragColor;

void main()
{
    vec2 uv = v_texCoord.xy;

    // skip empty space
    if (uv.x * u_size.x > 2.0 && uv.y * u_size.y > 2.0 && (1.0 - uv.y) * u_size.y > 2.0)
        discard;

    float alpha = 1.0;//max(0.0, 1.0 - 2.0 * abs(uv.x / width - 0.5)) + max(0.0, 1.0 - 2.0 * abs(uv.y / width - 0.5));

    fragColor.rgb = v_color.rgb;
    fragColor.a = alpha;
}