// assumes x>0
float erfc(float x) {
    return 2.0 * exp(-x * x) / (2.319 * x + sqrt(4.0 + 1.52 * x * x));
}

float erf(float x) {
    float a  = 0.140012;
    float x2 = x*x;
    float ax2 = a*x2;
    return sign(x) * sqrt( 1.0 - exp(-x2*(1.273239 + ax2)/(1.0 + ax2)) );
}

// From http://mathworld.wolfram.com/Erf.html
float erfFast( float z )
{
    float z2 = z*z;
    return 1.1283791670955125738961589031215 * z * (1.0 + z2 * ((-1.0/3.0) + z2 * ((1.0/10.0) + z2 * ((-1.0/42.0) + z2 * (1.0/216.0)))));
}

float getPiercingChance( float armor, float piercingMin, float piercingMax, vec4 platformData )
{
    if (armor <= piercingMin)
        return 1.0;
    if (armor >= piercingMax)
        return 0.0;
    //return 1.0 - ( armor - piercingMin ) / ( piercingMax - piercingMin );
    float avg = 0.5 * (piercingMin + piercingMax);
    float invscale = platformData.y * 2.0 / (piercingMax - piercingMin);

    // from the answers of the dev team
    // piercing power distribution is random.gauss(0, 0.25 * 0.33)
    // sigma is 0.25 * 0.33

    float invsigmasqrt2 = platformData.z;

    return 0.5 * (1.0 - erf((armor - avg) * invscale * invsigmasqrt2));
}

vec4 encodeFloatRGBA( float v )
{
  vec4 enc = vec4(1.0, 255.0, 65025.0, 16581375.0) * v;
  enc = fract(enc);
  enc -= enc.yzww * vec4(1.0/255.0,1.0/255.0,1.0/255.0,0.0);
  return enc;
}

float decodeFloatRGBA( vec4 rgba )
{
  return dot( rgba, vec4(1.0, 1.0/255.0, 1.0/65025.0, 1.0/16581375.0) );
}

vec2 encodeFloatRG( float v )
{
  vec2 enc = vec2(1.0, 255.0) * v;
  enc = fract(enc);
  enc.x -= enc.y * 1.0/255.0;
  return enc;
}

float decodeFloatRG( vec2 rg )
{
  return dot( rg, vec2(1.0, 1.0/255.0) );
}

float grayscale(vec3 col)
{
    return 0.3 * col.r + 0.59 * col.g + 0.11 * col.b;
}

//doesn’t strictly need to be all that accurate for this...
vec4 srgb2lin(vec4 c) { return vec4(c.rgb*c.rgb, c.a); }
vec4 lin2srgb(vec4 c) { return vec4(sqrt(c.rgb), c.a); }
vec3 srgb2lin(vec3 c) { return c.rgb*c.rgb; }
vec3 lin2srgb(vec3 c) { return sqrt(c.rgb); }




// Hash without Sine
// Creative Commons Attribution-ShareAlike 4.0 International Public License
// Created by David Hoskins.

// https://www.shadertoy.com/view/4djSRW
// Trying to find a Hash function that is the same on ALL systens
// and doesn't rely on trigonometry functions that lose accuracy different GPUs. 
// New one on the left, sine function on the right.
// It appears to be the same speed, but I suppose that depends.

// * Note. It still goes wrong eventually!
// * Try full-screen paused to see details.

// *** Change these to suit your range of random numbers..

// *** Use these for integer ranges, ie Value-Noise/Perlin functions.
//#define MOD3 vec3(.1031,.11369,.13787)
//#define MOD4 vec4(.1031,.11369,.13787, .09987)

// This set suits the coords of of 0-1.0 ranges..
#define MOD3 vec3(443.8975,397.2973, 491.1871)
#define MOD4 vec4(443.8975,397.2973, 491.1871, 470.7827)


//----------------------------------------------------------------------------------------
//  1 out, 1 in...
float hash11(float p)
{
	vec3 p3  = fract(vec3(p) * MOD3);
    p3 += dot(p3, p3.yzx + 19.19);
    return fract((p3.x + p3.y) * p3.z);
}

//----------------------------------------------------------------------------------------
//  1 out, 2 in...
float hash12(vec2 p)
{
	vec3 p3  = fract(vec3(p.xyx) * MOD3);
    p3 += dot(p3, p3.yzx + 19.19);
    return fract((p3.x + p3.y) * p3.z);
}

//----------------------------------------------------------------------------------------
//  1 out, 3 in...
float hash13(vec3 p3)
{
	p3  = fract(p3 * MOD3);
    p3 += dot(p3, p3.yzx + 19.19);
    return fract((p3.x + p3.y) * p3.z);
}

//----------------------------------------------------------------------------------------
//  2 out, 1 in...
vec2 hash21(float p)
{
	vec3 p3 = fract(vec3(p) * MOD3);
	p3 += dot(p3, p3.yzx + 19.19);
	return fract(vec2((p3.x + p3.y)*p3.z, (p3.x+p3.z)*p3.y));
}

//----------------------------------------------------------------------------------------
///  2 out, 2 in...
vec2 hash22(vec2 p)
{
	vec3 p3 = fract(vec3(p.xyx) * MOD3);
    p3 += dot(p3, p3.yzx+19.19);
    return fract(vec2((p3.x + p3.y)*p3.z, (p3.x+p3.z)*p3.y));
}

//----------------------------------------------------------------------------------------
///  2 out, 3 in...
vec2 hash23(vec3 p3)
{
	p3 = fract(p3 * MOD3);
    p3 += dot(p3, p3.yzx+19.19);
    return fract(vec2((p3.x + p3.y)*p3.z, (p3.x+p3.z)*p3.y));
}

//----------------------------------------------------------------------------------------
//  3 out, 1 in...
vec3 hash31(float p)
{
   vec3 p3 = fract(vec3(p) * MOD3);
   p3 += dot(p3, p3.yzx + 19.19);
   return fract(vec3((p3.x + p3.y)*p3.z, (p3.x+p3.z)*p3.y, (p3.y+p3.z)*p3.x));
}


//----------------------------------------------------------------------------------------
///  3 out, 2 in...
vec3 hash32(vec2 p)
{
	vec3 p3 = fract(vec3(p.xyx) * MOD3);
    p3 += dot(p3, p3.yxz+19.19);
    return fract(vec3((p3.x + p3.y)*p3.z, (p3.x+p3.z)*p3.y, (p3.y+p3.z)*p3.x));
}

//----------------------------------------------------------------------------------------
///  3 out, 3 in...
vec3 hash33(vec3 p3)
{
	p3 = fract(p3 * MOD3);
    p3 += dot(p3, p3.yxz+19.19);
    return fract(vec3((p3.x + p3.y)*p3.z, (p3.x+p3.z)*p3.y, (p3.y+p3.z)*p3.x));
}

//----------------------------------------------------------------------------------------
// 4 out, 1 in...
vec4 hash41(float p)
{
	vec4 p4 = fract(vec4(p) * MOD4);
    p4 += dot(p4, p4.wzxy+19.19);
    return fract(vec4((p4.x + p4.y)*p4.z, (p4.x + p4.z)*p4.y, (p4.y + p4.z)*p4.w, (p4.z + p4.w)*p4.x));
    
}

//----------------------------------------------------------------------------------------
// 4 out, 2 in...
vec4 hash42(vec2 p)
{
	vec4 p4 = fract(vec4(p.xyxy) * MOD4);
    p4 += dot(p4, p4.wzxy+19.19);
	return fract(vec4((p4.x + p4.y)*p4.z, (p4.x + p4.z)*p4.y, (p4.y + p4.z)*p4.w, (p4.z + p4.w)*p4.x));
}

//----------------------------------------------------------------------------------------
// 4 out, 3 in...
vec4 hash43(vec3 p)
{
	vec4 p4 = fract(vec4(p.xyzx) * MOD4);
    p4 += dot(p4, p4.wzxy+19.19);
	return fract(vec4((p4.x + p4.y)*p4.z, (p4.x + p4.z)*p4.y, (p4.y + p4.z)*p4.w, (p4.z + p4.w)*p4.x));
}

//----------------------------------------------------------------------------------------
// 4 out, 4 in...
vec4 hash44(vec4 p4)
{
	p4 = fract(p4 * MOD4);
    p4 += dot(p4, p4.wzxy+19.19);
    return fract(vec4((p4.x + p4.y)*p4.z, (p4.x + p4.z)*p4.y, (p4.y + p4.z)*p4.w, (p4.z + p4.w)*p4.x));
}




// http://lolengine.net/blog/2013/07/27/rgb-to-hsv-in-glsl

vec3 rgb2hsv(vec3 c)
{
    vec4 K = vec4(0.0, -1.0 / 3.0, 2.0 / 3.0, -1.0);
    vec4 p = c.g < c.b ? vec4(c.bg, K.wz) : vec4(c.gb, K.xy);
    vec4 q = c.r < p.x ? vec4(p.xyw, c.r) : vec4(c.r, p.yzx);

    float d = q.x - min(q.w, q.y);
    float e = 1.0e-10;
    return vec3(abs(q.z + (q.w - q.y) / (6.0 * d + e)), d / (q.x + e), q.x);
}

vec3 hsv2rgb(vec3 c)
{
    vec4 K = vec4(1.0, 2.0 / 3.0, 1.0 / 3.0, 3.0);
    vec3 p = abs(fract(c.xxx + K.xyz) * 6.0 - K.www);
    return c.z * mix(K.xxx, clamp(p - K.xxx, 0.0, 1.0), c.y);
}

// DeepSeek
// Convert RGB to CIEXYZ (simplified approximation)
vec3 rgbToXYZ(vec3 rgb) {
    mat3 rgbToXYZMatrix = mat3(
        0.4124564, 0.2126729, 0.0193339, // First column
        0.3575761, 0.7151522, 0.1191920, // Second column
        0.1804375, 0.0721750, 0.9503041  // Third column
    );
    return rgbToXYZMatrix * rgb;
}

// Convert CIEXYZ to RGB (simplified approximation)
vec3 xyzToRGB(vec3 xyz) {
    mat3 xyzToRGBMatrix = mat3(
        3.2404542, -0.9692660, 0.0556434,  // First column
        -1.5371385, 1.8760108, -0.2040259, // Second column
        -0.4985314, 0.0415560, 1.0572252   // Third column
    );
    return xyzToRGBMatrix * xyz;
}