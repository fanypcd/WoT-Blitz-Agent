#version 300 es
// fork of https://www.shadertoy.com/view/tdSXzD

uniform mat4 u_invViewProjectionMatrix;
uniform vec3 u_cameraPosition;
uniform vec3 u_lightDirection;
uniform float u_skyHeight;
uniform float u_cloudyFactor;
in vec2 a_position;

out vec3 v_worldPos;
out vec3 v_rayleigh;
out vec3 v_mie;

const int nSamples = 8;
const float fSamples = float(nSamples);

// atmosphere is 2.5% of planet radius
const float fInnerRadius = 1.0;

const float fScaleDepth = 0.25;

const vec3 invWavelengthLow = 1.0 / pow(vec3(0.66, 0.52, 0.47), vec3(4.0));
const vec3 invWavelengthHigh = 1.0 / pow(vec3(0.65, 0.58, 0.49), vec3(4.0));

const float Km = 0.0010;        // Mie scattering constant
const float Km4PI = Km*4.0*3.1415;

float scale(float fCos)
{
    float x = 1.0 - fCos;
    return fScaleDepth * exp(-0.00287 + x*(0.459 + x*(3.83 + x*(-6.80 + x*5.25))));
}

void main()
{
    float fOuterRadius = 1.025 + 0.1 * u_cloudyFactor;
    float fScale = 1.0 / (fOuterRadius - fInnerRadius);
    float fScaleOverScaleDepth = (1.0 / (fOuterRadius - fInnerRadius)) / fScaleDepth;

    gl_Position = vec4(a_position.x * 2.0 - 1.0, 1.0 - a_position.y * u_skyHeight, 0, 1);
    vec4 worldPos = u_invViewProjectionMatrix * gl_Position;
    v_worldPos = normalize(worldPos.xyz / worldPos.w - u_cameraPosition);      // ray to atmosphere

    vec3 lightDir = -u_lightDirection;

    float t = max(0.0, lightDir.y);
    float Kr = mix(0.0025, 0.0045, 1.0 - t * t);        // Rayleigh scattering constant
    float Kr4PI = Kr*4.0*3.1415;

    vec3 invWavelength = mix(invWavelengthLow, invWavelengthHigh, smoothstep(0.0, 0.5, t));

    // distance to outer atmosphere
    // ray-sphere intersection
    float k = v_worldPos.y * fInnerRadius;
    float c = fInnerRadius * fInnerRadius - fOuterRadius * fOuterRadius;
    float fFar = -k + sqrt(k * k - c);

    // Calculate the ray's starting position, then calculate its scattering offset
    vec3 v3Start = vec3(0.0, fInnerRadius, 0.0);
    float fHeight = fInnerRadius;
    float fDepth = 1.0;
    float fStartAngle = v_worldPos.y;
    float fStartOffset = fDepth*scale(fStartAngle);

    // Initialize the scattering loop variables
    float fSampleLength = fFar / fSamples;
    float fScaledLength = fSampleLength * fScale;
    vec3 v3SampleRay = v_worldPos * fSampleLength;
    vec3 v3SamplePoint = v3Start + v3SampleRay * 0.2;

    // Now loop through the sample rays
    vec3 color = vec3(0.0, 0.0, 0.0);
    for(int i=0; i<nSamples; i++)
    {
        float fHeight = length(v3SamplePoint);
        float fDepth = exp(fScaleOverScaleDepth * (fInnerRadius - fHeight));
        float fLightAngle = dot(lightDir, v3SamplePoint) / fHeight;
        float fCameraAngle = dot(v_worldPos, v3SamplePoint) / fHeight;
        float fScatter = (fStartOffset + fDepth*(scale(fLightAngle) - scale(fCameraAngle)));
        vec3 v3Attenuate = exp(-fScatter * (invWavelength * Kr4PI + Km4PI));
        color += v3Attenuate * (fDepth * fScaledLength);
        v3SamplePoint += v3SampleRay;
    }

    // Finally, scale the Mie and Rayleigh colors and set up the varying variables for the pixel shader
    v_mie = color * Km;
    v_rayleigh = color * (invWavelength * Kr);
}