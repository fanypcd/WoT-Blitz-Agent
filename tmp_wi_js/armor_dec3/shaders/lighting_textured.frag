#ifdef SIMPLE_LIGHTING

vec3 computeLighting(vec3 normalVector, vec3 lightDirection, vec3 lightColor, float attenuation, float specularValue)
{
    float diffuse = max(dot(normalVector, lightDirection), 0.0);
    vec3 diffuseColor = lightColor * _baseColor.rgb * diffuse * attenuation;

    #if defined(SPECULAR)

    // Blinn-Phong shading
    vec3 vertexToEye = normalize(v_cameraDirection);
    vec3 halfVector = normalize(lightDirection + vertexToEye);
    float specularAngle = clamp(dot(normalVector, halfVector), 0.0, 1.0);
    vec3 specularColor = vec3(pow(specularAngle, u_specularExponent)) * attenuation;

    return diffuseColor + specularColor * specularValue;

    #else
    
    return diffuseColor;
    
    #endif
}

vec3 getLitPixel()
{
    vec4 normalMapData = texture(u_normalmapTexture, v_texCoord);
    #if !defined(LEGACY_SHADER)
    #if defined(VERSION_HD)
    if (normalMapData.r < u_alphaRef)
        discard;
    #else
    if (normalMapData.b < u_alphaRef)
        discard;
    #endif
    #endif


    #if defined(BUMPED)
    
    #if defined(LEGACY_SHADER)
    vec3 normalVector = normalize(vec3(normalMapData.xy * 2.0 - 1.0, 0.5));
    // androids don't understand elif semantic sometime...
    #else
    #if defined(VERSION_HD)
    vec3 normalVector = normalize(vec3(normalMapData.ag * 2.0 - 1.0, 0.5));
    #else
    vec3 normalVector = normalize(vec3(normalMapData.rg * 2.0 - 1.0, 0.5));
    #endif
    #endif    
    
    #else
    
    vec3 normalVector = normalize(v_normalVector);
    
    #endif


    float specularValue = 1.0;

    
    vec3 combinedColor = vec3(0.0, 0.0, 0.0);

    // Directional light contribution
    #if (DIRECTIONAL_LIGHT_COUNT > 0)
    for (int i = 0; i < DIRECTIONAL_LIGHT_COUNT; ++i)
    {
        #if defined(BUMPED)
        vec3 lightDirection = normalize(v_directionalLightDirection[i]);
        #else
        vec3 lightDirection = normalize(u_directionalLightDirection[i]);
        #endif 
        combinedColor += computeLighting(normalVector, -lightDirection, u_directionalLightColor[i], 1.0, specularValue);
    }
    #endif

    // Point light contribution
    #if (POINT_LIGHT_COUNT > 0)
    for (int i = 0; i < POINT_LIGHT_COUNT; ++i)
    {
        vec3 ldir = v_vertexToPointLightDirection[i] * u_pointLightRangeInverse[i];
        float attenuation = clamp(1.0 - dot(ldir, ldir), 0.0, 1.0);
        combinedColor += computeLighting(normalVector, normalize(v_vertexToPointLightDirection[i]), u_pointLightColor[i], attenuation, specularValue);
    }
    #endif

    // Spot light contribution
    #if (SPOT_LIGHT_COUNT > 0)
    for (int i = 0; i < SPOT_LIGHT_COUNT; ++i)
    {
        // Compute range attenuation
        vec3 ldir = v_vertexToSpotLightDirection[i] * u_spotLightRangeInverse[i];
        float attenuation = clamp(1.0 - dot(ldir, ldir), 0.0, 1.0);
        vec3 vertexToSpotLightDirection = normalize(v_vertexToSpotLightDirection[i]);

        #if defined(BUMPED)
            vec3 spotLightDirection = normalize(v_spotLightDirection[i]);
        #else
            vec3 spotLightDirection = normalize(u_spotLightDirection[i]);
        #endif

        // "-lightDirection" is used because light direction points in opposite direction to spot direction.
        float spotCurrentAngleCos = dot(spotLightDirection, -vertexToSpotLightDirection);

        // Apply spot attenuation
        attenuation *= smoothstep(u_spotLightOuterAngleCos[i], u_spotLightInnerAngleCos[i], spotCurrentAngleCos);
        combinedColor += computeLighting(normalVector, vertexToSpotLightDirection, u_spotLightColor[i], attenuation, specularValue);
    }
    #endif

    return combinedColor;
}




#else // SIMPLE_LIGHTING



uniform vec2 u_shadowMapInvSize;

float computeShadow(sampler2D shadowMap, mat4 shadowMapVP)
{
    float shadowFactor = 1.0;

#if defined(USE_SHADOWS)
    vec4 shadowMapPoint = shadowMapVP * v_worldPoint;
    vec2 shadowMapUV = shadowMapPoint.xy * 0.5 + 0.5 * shadowMapPoint.w;
    vec2 shadowMapScale = shadowMapPoint.w * u_shadowMapInvSize;
    float testDepth = shadowMapPoint.z * 0.5 / shadowMapPoint.w + 0.5;

    // PCF filtering

    //*
    for(float ofsx = -0.5; ofsx < 1.0; ofsx += 1.0)
        for(float ofsy = -0.5; ofsy < 1.0; ofsy += 1.0)
        {
            vec4 shadowMapSample = textureProj(shadowMap, vec3(shadowMapUV + vec2(ofsx, ofsy) * shadowMapScale, shadowMapPoint.w));
            float shadowMapDepth = decodeFloatRGBA(shadowMapSample);
            if (testDepth > shadowMapDepth + 0.003)
                shadowFactor -= 0.25;
        }

    //*/

    // Variance shadow map (filtered)
    /*
    for(float ofsx = -0.5; ofsx < 1.0; ofsx += 1.0)
        for(float ofsy = -0.5; ofsy < 1.0; ofsy += 1.0)
        {
            vec4 shadowMapSample = textureProj(shadowMap, vec3(shadowMapUV + vec2(ofsx, ofsy) * shadowMapScale, shadowMapPoint.w));
            float shadowMapDepth = decodeFloatRGBA(shadowMapSample) + 0.0025;

            if (testDepth > shadowMapDepth)
            {
                float depthScale = 10.0;
                float moment2 = decodeFloatRG(shadowMapSample.ba);
                float shadowMapVariance = max(0.0125, (moment2 - shadowMapDepth * shadowMapDepth) * depthScale * depthScale);
                float d = (testDepth - shadowMapDepth) * depthScale;
                float pmax = shadowMapVariance / (shadowMapVariance + d * d);
                shadowFactor -= (1.0 - pmax) * 0.25;
            }
        }
    //*/
#endif

    return shadowFactor;
}





//
// Source: https://www.shadertoy.com/view/ls3Szr
// Modified by Andrew Karpushin
//

const float MIPMAP_SWITCH  = 0.29; //sampling angle delta (rad) equivalent to the lowest LOD.

//http://www.filmetrics.com/refractive-index-database/Al/Aluminium
//http://refractiveindex.info/?shelf=3d&book=liquids&page=water
//https://seblagarde.wordpress.com/2011/08/17/feeding-a-physical-based-lighting-mode/
const vec3  F_ALU_N  = vec3(1.600,0.912,0.695); //(Red ~ 670 nm; Green ~ 540 nm; Blue ~ 475 nm)
const vec3  F_ALU_K  = vec3(8.010,6.500,5.800); //(Red ~ 670 nm; Green ~ 540 nm; Blue ~ 475 nm)
const vec3  F_GOLD_N = vec3(0.161,0.402,1.242); //(Red ~ 670 nm; Green ~ 540 nm; Blue ~ 475 nm)
const vec3  F_GOLD_K = vec3(3.446,2.540,1.796); //(Red ~ 670 nm; Green ~ 540 nm; Blue ~ 475 nm)
const vec3  F_IRON_N = vec3(2.905,2.921,2.681); //(Red ~ 670 nm; Green ~ 540 nm; Blue ~ 475 nm)
const vec3  F_IRON_K = vec3(3.122,2.923,2.829); //(Red ~ 670 nm; Green ~ 540 nm; Blue ~ 475 nm)
const float F_DIELECTRIC_PLASTIC = 1.49; //@550nm, does not change much with wavelength for dielectric
const float F_DIELECTRIC_WATER   = 1.33; //@550nm
const float F_DIELECTRIC_DIAMOND = 2.42; //@550nm


//Arbitrary axis rotation (around u, normalized)
mat3 UTIL_axisRotationMatrix( vec3 u, float t )
{
    float c = cos(t);
    float s = sin(t);
    //  _        _   _           _     _                    _ 
    // |_px py pz_| | m11 m21 m31 |   | px*m11+py*m21+pz*m31 |
    //              | m12 m22 m32 | = | px*m12+py*m22+pz*m32 |
    //              |_m13 m23 m33_|   |_px*m13+py*m23+pz*m33_|
    return mat3(  c+u.x*u.x*(1.-c),     u.x*u.y*(1.-c)-u.z*s, u.x*u.z*(1.-c)+u.y*s,
                  u.y*u.x*(1.-c)+u.z*s, c+u.y*u.y*(1.-c),     u.y*u.z*(1.-c)-u.x*s,
                  u.z*u.x*(1.-c)-u.y*s, u.z*u.y*(1.-c)+u.x*s, c+u.z*u.z*(1.-c) );
}

#if defined(SPECULAR)
vec3 PBR_HDRCubemap(vec3 sampleDir, float LOD_01)
{
    /*
    vec3 linearGammaColor_sharp = PBR_HDRremap(pow(texture( iChannel2, sampleDir ).rgb,vec3(2.2)));
    vec3 linearGammaColor_blur  = PBR_HDRremap(pow(texture( iChannel3, sampleDir ).rgb,vec3(1)));
    vec3 linearGammaColor = mix(linearGammaColor_sharp,linearGammaColor_blur,saturate(LOD_01));
    return linearGammaColor;
    */

    float skyValue = 0.5 - dot(sampleDir, v_upDirection) * 0.5;
    return texture(u_skySpecularTexture, vec2(LOD_01, skyValue)).rgb;
}
#endif

vec3 PBR_nudgeSample(vec3 sampleDir, float roughness, float e1, float e2, out float range)
{
    const float PI = 3.14159;
    //Importance sampling :
    //Source : http://www.codinglabs.net/article_physically_based_rendering_cook_torrance.aspx
    //The higher the roughness, the broader the range.
    //In any case, wide angles are less probable than narrow angles.
    range = atan( roughness*sqrt(e1)/sqrt(1.0-e1) );
    //Circular angle has an even distribution (could be improved?).
    float phi = 2.0*PI*e2;
    
    vec3 up = vec3(0,1,0); //arbitrary
    vec3 tAxis = cross(up,sampleDir);
    mat3 m1 = UTIL_axisRotationMatrix(normalize(tAxis),range);
    mat3 m2 = UTIL_axisRotationMatrix(normalize(sampleDir), phi);
        
    return sampleDir*m1*m2;
}

//PBR Equation for a single sample (IBL) or a single point point (ABL)
vec3 PBR_Equation(vec3 V, vec3 L, vec3 N, float roughness, vec3 ior_n, vec3 ior_k, bool bIBL)
{
    vec3 H = normalize(L+V);
    float NdotH = dot(N,H);//Nn.H;
    if (NdotH < 0.0 && !bIBL)
        return vec3(0.0);

    float cosT = max(dot(L, N), 0.0);
    
    float NdotL = cosT;//Nn.Ln;
    float VdotH = dot(V,H);//Vn.H;
    float NdotV = max(0.0, dot(N,V));//Nn.Vn;

    //-----------------------------------------
    //            Distribution Term
    //-----------------------------------------
    float PI = 3.14159;
    float alpha2 = roughness * roughness;
    float NoH2 = NdotH * NdotH;
    float den = NoH2*(alpha2-1.0)+1.0;
    float D = 1.0; //Distribution term is externalized from IBL version
    if(!bIBL)
        D = alpha2/(PI*den*den); //GGX Distribution.
    
    //-----------------------------------------
    //            Fresnel Term
    //-----------------------------------------
    vec3 F;

    //if(metallic)
    //{
        //Source: http://sirkan.iit.bme.hu/~szirmay/fresnel.pdf p.3 above fig 5
        float cos_theta = NdotV;
        F =  ((ior_n-1.)*(ior_n-1.)+ior_k*ior_k+4.*ior_n*pow(1.-cos_theta,5.))
            /((ior_n+1.)*(ior_n+1.)+ior_k*ior_k);
    //}
    //else
    //{
    //    //Fresnel Schlick Dielectric formula 
    //    //Sources: https://en.wikipedia.org/wiki/Schlick%27s_approximation
    //    //          http://www.codinglabs.net/article_physically_based_rendering_cook_torrance.aspx
    //    //Note: R/G/B do not really differ for dielectric materials
    //    float F0 = abs ((1.0 - ior_n.x) / (1.0 + ior_n.x));
    //    F = vec3(F0 + (1.-F0) * pow( 1. - NdotV, 5.));
    //}
    
    //-----------------------------------------
    //            Geometric term
    //-----------------------------------------
    //Source: Real Shading in Unreal Engine 4 2013 Siggraph Presentation
    //https://de45xmedrsdbp.cloudfront.net/Resources/files/2013SiggraphPresentationsNotes-26915738.pdf p.3/59
    //k = Schlick model (IBL) : Disney's modification to reduce hotness (point light)
    float k;
    float cap;
    if (bIBL)
    {
        k = (roughness+1.)*(roughness+1.)/8.;
        cap = 1.0;
    }
    else
    {
        k = roughness/2.0;
        cap = 4.0;
    }
    float Gl = NdotL*(1.0-k)+k;
    float Gv = NdotV*(1.0-k)+k;
    float G = min(cap, 1.0/(Gl*Gv));  // divide by NdotL and NdotV right here, using cap to remove too much specular at grazing angles
    
    //-----------------------------------------
    //     PBR Equation (ABL & IBL versions)
    //-----------------------------------------
    //Two flavors of the PBR equation (IBL/point light).
    //Source: http://www.codinglabs.net/article_physically_based_rendering_cook_torrance.aspx
    if(bIBL)
        return 0.25*F*G*sqrt(1.0 - cosT * cosT); //IBL
    return 0.25*D*F*G;    //ABL
}

#if defined(SPECULAR)

vec3 PBR_visitSamplesLUT(vec3 V, vec3 N, float roughness, vec3 ior_n, vec3 ior_k )
{
    //Direct relection vector
    vec3 vCenter = reflect(-V,N);

    // Assuming environment cubemap is rotation-symmetrical so we need 2D LUT texture to get sky color depending on roughness
    vec3 sampleColor = PBR_HDRCubemap(vCenter, roughness);
    vec3 contribution = PBR_Equation(V, vCenter, N, roughness, ior_n, ior_k, true);
    return sampleColor * contribution;
}

vec3 PBR_visitSamples(vec3 V, vec3 N, float roughness, vec3 ior_n, vec3 ior_k )
{
    //Direct relection vector
    vec3 vCenter = reflect(-V,N);
    
    //------------------------------------------------
    //  Randomized Samples : more realistic, but
    //  a lot of samples before it stabilizes 
    //------------------------------------------------
    float randomness_range = 0.75; //Cover only the closest 75% of the distribution. Reduces range, but improves stability.
    float fIdx = 0.0;              //valid range = [0.5-1.0]. Note : it is physically correct at 1.0.
    const int ITER_RDM = 1;
    const float w_rdm = 1.0/float(ITER_RDM);
    vec3 totalRandom = vec3(0.0);
    for(int i=0; i < ITER_RDM; ++i)
    {
        //Random jitter note : very sensitive to hash quality (patterns & artifacts).
        vec2 jitter = hash22(fIdx*10.0+vCenter.xy*100.0);
        float angularRange = 0.;    
        vec3 sampleDir    = PBR_nudgeSample(vCenter, roughness, jitter.x*randomness_range, jitter.y, angularRange);
        vec3 sampleColor  = PBR_HDRCubemap( sampleDir, angularRange/MIPMAP_SWITCH);
        vec3 contribution = PBR_Equation(V, sampleDir, N, roughness, ior_n, ior_k, true)*w_rdm;
        totalRandom += contribution*sampleColor;
        ++fIdx;
    }
    
    //------------------------------------------------
    //  Fixed Samples : More stable, but creates
    //  sampling pattern artifacts and the reach is
    //  limited.
    //------------------------------------------------
    fIdx = 0.0;
    const int ITER_FIXED = 1;
    const float w_fixed = 1.0/float(ITER_FIXED); //Sample
    vec3 totalFixed = vec3(0.0);
    for(int i=0; i < ITER_FIXED; ++i)
    {
        //Stable pseudo-random jitter (to improve stability with low sample count)
        //Beware here! second component controls the sampling pattern "swirl", and it must be choosen 
        //             so that samples do not align by doing complete 360deg cycles at each iteration.
        vec2 jitter = vec2( clamp(w_fixed*fIdx,0.0,0.50),
                            fract(w_fixed*fIdx*1.25)+3.14*fIdx);
        float angularRange = 0.;
        vec3 sampleDir    = PBR_nudgeSample(vCenter, roughness, jitter.x, jitter.y, angularRange);
        vec3 sampleColor  = PBR_HDRCubemap( sampleDir, angularRange/MIPMAP_SWITCH);
        vec3 contribution = PBR_Equation(V, sampleDir, N, roughness, ior_n, ior_k, true)*w_fixed;
        totalFixed += contribution*sampleColor;
        ++fIdx;
    }
    
    return (totalRandom*float(ITER_RDM)+totalFixed*float(ITER_FIXED))/(float(ITER_RDM)+float(ITER_FIXED));
}

#endif

vec3 computeLightingPBR(vec3 V, vec3 N, vec3 L, vec3 lightColor, float attenuation, float roughness, vec3 ior_N, vec3 ior_K)
{
    return lightColor * PBR_Equation(V,L,N, roughness, ior_N, ior_K, false);
}




vec3 computeLighting(vec3 normalVector, vec3 lightDirection, vec3 lightColor, float attenuation, float specularValue, float specularPower, float shadow)
{
    float NdotL = dot(normalVector, lightDirection);
    
    float diffuse = max(NdotL, 0.0);
    vec3 baseColor = lightColor * diffuse * attenuation;
    vec3 diffuseColor = baseColor * _baseColor.rgb;
    float u_specularExponent = 50.0;

    #if defined(SPECULAR)

    // Phong shading
    vec3 vertexToEye = normalize(v_cameraDirection);
    vec3 specularColor = vec3(0.0, 0.0, 0.0);
    if (NdotL > 0.0)
    {
        vec3 specularAngle = normalVector * diffuse * 2.0 - lightDirection;  
        specularColor = pow(clamp(dot(specularAngle, vertexToEye), 0.0001, 1.0), u_specularExponent) * lightColor * 0.5 * shadow;

        vec3 reflectedRay = normalVector * dot(normalVector, vertexToEye) * 2.0 - vertexToEye;
        float skyValue = 0.5 - dot(reflectedRay, v_upDirection) * 0.5;
        specularColor += texture(u_skySpecularTexture, vec2(0.5, skyValue)).rgb * specularPower * (0.2 + 0.8 * shadow);
    }

    /*
    float fresnel = dot(vertexToEye, normalVector);
    float rimMask = clamp((0.5 - fresnel) * 2.0, 0.0, 1.0);
    rimMask *= rimMask;
    rimMask *= rimMask;
    rimMask *= specularValue;
    */

    // Blinn-Phong shading
    //vec3 vertexToEye = normalize(v_cameraDirection);
    //vec3 halfVector = normalize(lightDirection + vertexToEye);
    //float specularAngle = clamp(dot(normalVector, halfVector), 0.0, 1.0);
    //vec3 specularColor = vec3(pow(specularAngle, u_specularExponent)) * attenuation;

    //return mix(diffuseColor + specularColor * lightColor * specularValue, baseColor, rimMask);

    return diffuseColor;// * (1.0 - specularPower) * shadow + specularColor * specularValue;

    #else
    
    return diffuseColor;
    
    #endif
}


vec3 getLitPixel()
{
    #if defined(BUMPED)
    
    vec4 normalMapData = texture(u_normalmapTexture, v_texCoord);

    #if defined(LEGACY_SHADER)
    vec3 normalVector = normalize(vec3(normalMapData.xy * 2.0 - 1.0, 0.5));
    // androids don't understand elif semantic sometimes...
    #else
    #if defined(VERSION_HD)
    if (normalMapData.r < u_alphaRef)
        discard;
    vec3 normalVector = normalize(vec3(normalMapData.ag * 2.0 - 1.0, 0.5));
    #else
    if (normalMapData.b < u_alphaRef)
        discard;
    vec3 normalVector = normalize(vec3(normalMapData.rg * 2.0 - 1.0, 0.5));
    #endif
    #endif    
    
    #else
    
    vec3 normalVector = normalize(v_normalVector);
    
    #endif



    #if defined(DETAIL_MAP)
    vec3 detailMap = texture(u_detailTexture, v_texCoord * u_detailTiling).rgb;  // roughness, albedo, specular
    #else
    vec3 detailMap = vec3(0.5);
    #endif

    #if defined(SPECULAR)

    vec4 glossMap = texture(u_metallicGlossTexture, v_texCoord);

    // glossMap.r - specular: 0 for clothes, 0.5 for metals, 1.0 for mirrors
    // glossMap.g - gloss: 1 on metallic parts, 0.5 for hull
    // glossMap.b - camo mask / dirt

    #if defined(LEGACY_SHADER)
    float specularValue = glossMap.r;
    float roughness = 0.3;
    #else
    float specularValue = glossMap.r * detailMap.b * 2.0;
    float roughness = max(0.1, 1.21 - glossMap.g * 3.2 * detailMap.r * 2.0); // 3.3 since the average color for metallic parts is 0.25 and we need some room for absolute mirrors
    #endif

    #else

    float specularValue = 0.0;
    float roughness = 0.3;

    #endif

    vec3 albedo = _baseColor.rgb;

    #if defined(AMBIENT_OCCLUSION) && !defined(LEGACY_SHADER)
    #if defined(VERSION_HD)
    vec2 aoTex = texture(u_aoTexture, v_texCoord).rg;
    float camoMask = glossMap.b;
    float ambientOcclusion = aoTex.g;

    vec4 camoTex = texture(u_camoTexture, v_texCoord * u_camoTiling.xy + u_camoTiling.zw);
    vec4 camoColor = camoTex.r * u_camoPalette0 + camoTex.g * u_camoPalette1 + camoTex.b * u_camoPalette2 + camoTex.a * u_camoPalette3;

    albedo = mix(albedo, u_paintColor.rgb, u_paintColor.a * camoMask);
    albedo = mix(albedo, camoColor.rgb, camoColor.a * camoMask);

    // apply color shift for paint under sun light
    float paintUVexposure = ambientOcclusion * ambientOcclusion * glossMap.b * u_paintFade;
    paintUVexposure *= paintUVexposure * max(0.0, dot(normalVector, v_upDirection));

    vec3 hsv = rgb2hsv(albedo);
    hsv.x = fract(hsv.x + 1.0 - 1.0 / 10.0 * paintUVexposure);
    hsv.y = max(0.0, hsv.y - 0.4 * paintUVexposure);
    hsv.z = min(0.8, hsv.z + 0.6 * paintUVexposure);
    albedo = hsv2rgb(hsv);

    #else
        #if defined(SPECULAR)
        float ambientOcclusion = glossMap.b;    // u_metallicGlossTexture is already sampled
        #else
        float ambientOcclusion = texture(u_metallicGlossTexture, v_texCoord).b;
        #endif
    #endif
    #else
    float ambientOcclusion = 1.0;
    #endif


    
    #if defined(USE_SHADOWS)
    float shadowFactor = computeShadow(u_shadowMap, u_shadowMapViewProjection[0]);// * ambientOcclusion;
    #else
    float shadowFactor = 1.0;
    #endif

    //-----------------------------------------
    //         IBL and ABL PBR Lighting
    //-----------------------------------------
    vec3 ior_N = F_IRON_N;
    vec3 ior_K = F_IRON_K;


    vec3 diffuseContribution = vec3(0.0);
    vec3 N = normalVector;
    #if defined(SPECULAR)
    vec3 V = normalize(v_cameraDirection);
    vec3 specularContribution = PBR_visitSamplesLUT(V, N, roughness, ior_N, ior_K);   // small hack to make specular environment lights appear more
    #if defined(VERSION_HD)
    vec3 ambientLight = mix(texture(u_skySpecularTexture, vec2(1.0, (1.0 - dot(N, v_upDirection)) * 0.5)).rgb, vec3(1.0), u_cloudyFactor);
    #else
    vec3 ambientLight = texture(u_skySpecularTexture, vec2(1.0, (1.0 - dot(N, v_upDirection)) * 0.5)).rgb;
    #endif
    vec3 irradianceContribution = ambientLight * u_ambientColor;
    #else
    vec3 V = normalVector;
    vec3 specularContribution = vec3(0.0);
    vec3 irradianceContribution = u_ambientColor;
    #endif


    // Directional light contribution
    #if (DIRECTIONAL_LIGHT_COUNT > 0)
    #if defined(BUMPED)
    vec3 lightDirection = -normalize(v_directionalLightDirection[0]);
    #else
    vec3 lightDirection = -normalize(u_directionalLightDirection[0]);
    #endif

    shadowFactor *= max(0.0, dot(lightDirection, N));   // needed to remove specular on backfaces

    #if defined(VERSION_HD) && defined(SPECULAR)
    float dayTime = dot(v_upDirection, lightDirection);
    specularContribution *= (0.5 + 0.5 * dayTime) * (1.0 - u_cloudyFactor); // here we simulate the intensity of the sky, specularContribution for now only contains the specular from the sky
    #endif

    diffuseContribution = u_directionalLightColor[0] * shadowFactor;
    specularContribution += computeLightingPBR(V, N, lightDirection, u_directionalLightColor[0], 1.0, roughness, ior_N, ior_K) * shadowFactor;
    for (int i = 1; i < DIRECTIONAL_LIGHT_COUNT; ++i)
    {
        #if defined(BUMPED)
        vec3 lightDirection = -normalize(v_directionalLightDirection[i]);
        #else
        vec3 lightDirection = -normalize(u_directionalLightDirection[i]);
        #endif

        shadowFactor = max(0.0, dot(lightDirection, N));

        diffuseContribution += u_directionalLightColor[i] * shadowFactor;

        // don't show specular highlights from second light source on tracks
        #if !defined(TRACKS)
        specularContribution += computeLightingPBR(V, N, lightDirection, u_directionalLightColor[i], 1.0, roughness, ior_N, ior_K) * shadowFactor;
        #endif
    }
    #endif

    // Point light contribution
    #if (POINT_LIGHT_COUNT > 0)
    for (int i = 0; i < POINT_LIGHT_COUNT; ++i)
    {
        vec3 ldir = v_vertexToPointLightDirection[i] * u_pointLightRangeInverse[i];
        float attenuation = clamp(1.0 - dot(ldir, ldir), 0.0, 1.0);
        diffuseContribution += u_pointLightColor[i] * max(0.0, dot(lightDirection, N)) * attenuation;
        specularContribution += computeLighting(normalVector, normalize(v_vertexToPointLightDirection[i]), u_pointLightColor[i], attenuation, specularValue, roughness, ambientOcclusion);
    }
    #endif

    // Spot light contribution
    #if (SPOT_LIGHT_COUNT > 0)
    for (int i = 0; i < SPOT_LIGHT_COUNT; ++i)
    {
        // Compute range attenuation
        vec3 ldir = v_vertexToSpotLightDirection[i] * u_spotLightRangeInverse[i];
        float attenuation = clamp(1.0 - dot(ldir, ldir), 0.0, 1.0);
        vec3 vertexToSpotLightDirection = normalize(v_vertexToSpotLightDirection[i]);

        #if defined(BUMPED)
            vec3 spotLightDirection = normalize(v_spotLightDirection[i]);
        #else
            vec3 spotLightDirection = normalize(u_spotLightDirection[i]);
        #endif

        // "-lightDirection" is used because light direction points in opposite direction to spot direction.
        float spotCurrentAngleCos = dot(spotLightDirection, -vertexToSpotLightDirection);

        // Apply spot attenuation
        attenuation *= smoothstep(u_spotLightOuterAngleCos[i], u_spotLightInnerAngleCos[i], spotCurrentAngleCos);
        diffuseContribution += u_spotLightColor[i] * max(0.0, dot(lightDirection, N)) * attenuation;
        specularContribution += computeLighting(normalVector, vertexToSpotLightDirection, u_spotLightColor[i], attenuation, specularValue, roughness, ambientOcclusion);
    }
    #endif
 
    vec3 combinedColor = albedo * diffuseContribution * detailMap.g * 2.0;
    #if defined(SPECULAR)
    combinedColor += specularValue * specularContribution;
    #endif

    // double AO to have more contrast
    return (combinedColor + irradianceContribution * albedo) * ambientOcclusion;
}


#endif // SIMPLE_LIGHTING