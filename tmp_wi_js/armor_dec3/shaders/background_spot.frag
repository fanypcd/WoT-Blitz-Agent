#version 300 es
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif


#ifdef HE_SHELL

uniform vec2 u_depthConstants;
uniform mat4 u_viewMatrix;

#else

uniform vec4 u_color0;
uniform vec4 u_color1;
uniform vec4 u_color2;
uniform vec4 u_color3;
#if !defined(BACK)
uniform sampler2D u_shadowMap;
uniform mat4 u_shadowMapViewProjection0;
#endif

#ifdef HEDAMAGE
uniform sampler2D u_damageRamp;
uniform sampler2D u_damageTexture;
in vec3 v_screenCoord;
#endif

#ifdef RAMMING
uniform float u_targetSpeed;
uniform float u_rammerSpeed;
uniform vec3 u_cameraWorldPosition;
in vec4 v_groundVehicleLocalPosition;
#endif

const float radius = 3.0;

#endif

in vec4 v_worldPosition;
in vec2 v_texCoord;

out vec4 fragColor;

#include "shared.frag"


vec3 drawArrows(vec2 speed, float speedAbs, vec3 color0, vec3 arrowColor)
{
    if (speed.x > 0.0 && speed.x < 1.0 && speed.y > 0.0 && speed.y < speedAbs)
    {
        speed.x *= 2.0;
        speed.y = fract(speed.y);
        float d1 = speed.y - speed.x;
        float d2 = speed.y - 2.0 + speed.x;
        if (d1 < 0.0 && d2 < 0.0)
            color0 = arrowColor;
    }
    return color0;
}


#if defined(PLATFORM_WIN32) || defined(PLATFORM_EMSCRIPTEN) || defined(VERSION_HD)

#define NUM_CELLS 1.0

vec2 get_cell_point(vec2 cell) {
    return (cell + hash22(cell)) / NUM_CELLS;
}

float pattern(vec2 coord)
{
    vec2 cell = floor(vec2(coord * NUM_CELLS));
    float dist = 1.0;
    vec2 dist_cell;
    vec2 dist2_cell;
    
    for (int x = -1; x <=1; x++) { 
        for (int y = -1; y <=1; y++) {
        	vec2 cell_point = get_cell_point(cell + vec2(x, y)) - coord;
            float len = dot(cell_point, cell_point);
            if (len < dist)
            {
                dist = len;
                dist_cell = cell_point;
            }
        }
    }

    dist = 1.0;
    float dist2 = 1.0;
    for (int x = -1; x <=1; x++) { 
        for (int y = -1; y <=1; y++) {
        	vec2 cell_point = get_cell_point(cell + vec2(x, y)) - coord;

            dist2_cell = cell_point;
            vec2 n = dist2_cell - dist_cell;
            float nlenSq = dot(n, n);
            if (nlenSq > 0.0)
                dist = min(dist, (dot(n, dist_cell + dist2_cell) / nlenSq) * (0.3 + dist * 2.55));
        }
    }
    
    //dist = smoothstep(0.05, 0.1, dist);
    dist = step(0.05, dist);
    return dist;
}

#else

float pattern(vec2 uv)
{
    return hash12(floor(uv * 2.0));
}

#endif

void main()
{
#ifdef HE_SHELL

    float viewDistance = (u_viewMatrix * v_worldPosition).z;
    fragColor = vec4(0.0, 0.0, 0.0, (-viewDistance - u_depthConstants.x) / u_depthConstants.y);

#else

    vec2 uv = (v_texCoord - vec2(0.5, 0.5)) * 2.0;

    float rSq = dot(uv, uv);
    if (rSq > 1.0)
        discard;

    rSq = sqrt(rSq);

    vec4 color0 = u_color0;
    vec4 color1 = u_color1;
    vec4 color2 = u_color2;
    vec4 color3 = u_color3;

    // draw circle
    float w = 0.05;
    float r = dot(v_worldPosition.xz, v_worldPosition.xz);

#ifdef BACK
    if (r > (radius + w) * (radius + w))
        color0.a = 0.0;
    else
        color0.a = 0.33;
#endif

    if (rSq < 0.2)
        color0.rgb = mix(color0.rgb, color1.rgb, smoothstep(0.0, 0.2, rSq));
    else if (rSq < 0.47)
        color0.rgb = mix(color1.rgb, color2.rgb, smoothstep(0.2, 0.47, rSq));
    else
        color0.rgb = mix(color2.rgb, color3.rgb, smoothstep(0.47, 1.0, rSq));

    // draw circle
    if (r < (radius + w) * (radius + w) && r > (radius - w) * (radius - w))
    {
        color0.rgb *= vec3(0.8);
    }
    else
    {
        float patternFactor = 0.3 * r / (radius * radius);
        if (patternFactor < 1.0)
        {
            color0.rgb *= 1.0 - 0.15 * (1.0 - pattern(v_worldPosition.xz)) * (1.0 - patternFactor);
        }

#ifdef RAMMING
        // draw target speed arrows
        vec2 speed;
        speed.x = 1.5 * v_groundVehicleLocalPosition.x + 0.5;
        float speedAbs = abs(u_targetSpeed);
        if (v_groundVehicleLocalPosition.z > 0.0)
            speed.y = v_groundVehicleLocalPosition.z - radius;
        else
            speed.y = v_groundVehicleLocalPosition.z + radius;
        speed.y *= 2.5;
        if (u_targetSpeed < 0.0)
            speed.y = -speed.y;
        color0.rgb = drawArrows(speed, speedAbs, color0.rgb, vec3(0.4, 0.4, 0.4));

        // draw rammer speed arrows
        vec2 toCamera = normalize(vec2(u_cameraWorldPosition.x, u_cameraWorldPosition.z));
        vec2 tangent = vec2(toCamera.y, -toCamera.x);
        speed.x = 1.5 * dot(tangent, v_worldPosition.xz) + 0.5;
        speed.y = dot(toCamera, v_worldPosition.xz) - radius;
        speed.y = u_rammerSpeed - speed.y * 2.5;
        color0.rgb = drawArrows(speed, u_rammerSpeed, color0.rgb, vec3(0.4, 0.8, 0.4));
#endif
    }

#if !defined(BACK)
    vec4 shadowMapPlanePoint = u_shadowMapViewProjection0 * v_worldPosition;
    vec2 shadowMapUV = shadowMapPlanePoint.xy / shadowMapPlanePoint.w * 0.5 + vec2(0.5, 0.5);
    vec4 depthSample = texture(u_shadowMap, shadowMapUV);
    if (depthSample.r * depthSample.g * depthSample.b * depthSample.a < 1.0)    // check that depth buffer has values other than 1.0
        color0.rgb *= 0.75;
#endif

#ifdef HEDAMAGE
    vec2 normalizedScreenCoord = v_screenCoord.xy / v_screenCoord.z;
    vec4 data = texture(u_damageTexture, normalizedScreenCoord);
    vec3 res = texture(u_damageRamp, vec2(data.g,0.5)).rgb;
    
    color0.rgb = mix(color0.rgb, res, data.g);
#endif

    fragColor.rgb = color0.rgb + hash32(v_texCoord)/255.0;
    fragColor.a = color0.a;

#endif
}