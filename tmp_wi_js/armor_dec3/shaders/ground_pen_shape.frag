#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

in vec3 v_texCoord;

out vec4 fragColor;

void main()
{
    float maxDmg = max(v_texCoord.y, v_texCoord.z);
    float penFactor = v_texCoord.x * maxDmg / (v_texCoord.y + 0.001);
    float bounceFactor = v_texCoord.x * maxDmg / (v_texCoord.z + 0.001);
    penFactor = step(penFactor, 1.0);
    bounceFactor = step(bounceFactor, 1.0);

    vec3 color = vec3(0.0);
    color.g = penFactor;
    color.r = bounceFactor;

    fragColor.rgb = color * v_texCoord.x;
    fragColor.a = v_texCoord.x;
}