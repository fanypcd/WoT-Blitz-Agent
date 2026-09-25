#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

#include "lighting_header.frag"

uniform float u_moduleType;
uniform float u_saturation;

out vec4 fragColor;

#include "shared.frag"

#if defined(GUN_INTERNAL)
in float v_positionZLocal;
#endif

#if defined(TURRET_RING_STRIP)
in float v_normalYLocal;
#endif

#if defined(CLIP_PLANE) || defined(CLIP_PLANE_INVERSE)
in float v_clipDistance;
#endif

void main()
{
#if defined(CLIP_PLANE)
    if (v_clipDistance < 0.0)
        discard;
#endif

#if defined(CLIP_PLANE_INVERSE)
    if (v_clipDistance > 0.0)
        discard;
#endif

    #if defined(GUN_INTERNAL)
    if (v_positionZLocal > 0.5)
        discard;
    #endif

    #if defined(TURRET_RING_STRIP)
    if (abs(v_normalYLocal) > 0.3)
        discard;
    #endif

    _baseColor = texture(u_diffuseTexture, vec2(u_moduleType,0.5));

#if defined(CROSS_SECTION)
    float gray = 0.5;
    _baseColor.rgb = mix(vec3(gray, gray, gray), _baseColor.rgb, step(0.5, u_saturation));
#else
    float gray = grayscale(_baseColor.rgb);
    _baseColor.rgb = mix(vec3(gray, gray, gray), _baseColor.rgb, u_saturation);
#endif
    fragColor.a = _baseColor.a;

    #if defined(TEXTURE_DISCARD_ALPHA)
    if (fragColor.a < 0.5)
        discard;
    #endif

    #if defined(LIGHTING)

    _baseColor = srgb2lin(_baseColor);
    vec3 ambientColor = _baseColor.rgb * u_ambientColor;
    vec4 res = lin2srgb(vec4(ambientColor + getLitPixel(), 1.0));
    fragColor.rgb = res.rgb;
    #else
    fragColor.rgb = _baseColor.rgb;
    #endif

    #if defined(LIGHTMAP)
    vec4 lightColor = texture(u_lightmapTexture, v_texCoord1);
    fragColor.rgb *= lightColor.rgb;
    #endif

    #if defined(MODULATE_COLOR)
    fragColor *= u_modulateColor;
    #endif

    #if defined(MODULATE_ALPHA)
    fragColor.a *= u_modulateAlpha;
    #endif
}