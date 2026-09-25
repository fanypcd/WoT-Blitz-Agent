#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

///////////////////////////////////////////////////////////
// Uniforms
uniform sampler2D u_texture;

#ifdef DISTANCE_FIELD
uniform vec2 u_cutoff;
#endif

///////////////////////////////////////////////////////////
// Varyings
in vec2 v_texCoord;
in vec4 v_color;

out vec4 fragColor;

void main()
{ 
    #ifdef DISTANCE_FIELD
    
    fragColor = v_color;
    float distance = texture(u_texture, v_texCoord).a;
    float smoothing = fwidth(distance);
    float alpha = smoothstep(0.5 - smoothing * u_cutoff.x, 0.5 + smoothing * u_cutoff.y, distance);
    fragColor.a = alpha * v_color.a;
    
    #else
    
    fragColor = v_color;
    fragColor.a = texture(u_texture, v_texCoord).a * v_color.a;
    
    #endif
}