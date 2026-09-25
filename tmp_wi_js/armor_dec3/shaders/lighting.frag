vec3 computeLighting(vec3 normalVector, vec3 lightDirection, vec3 lightColor, float attenuation, float specularValue)
{
    float nDotL = dot(normalVector, lightDirection);
#ifdef BACK_LIGHT
    float diffuse = abs(nDotL);
#else
    float diffuse = max(nDotL, 0.0);
#endif
    vec3 diffuseColor = lightColor * _baseColor.rgb * diffuse * attenuation;

    #if defined(SPECULAR)

    // Phong shading
    //vec3 vertexToEye = normalize(v_cameraDirection);
    //vec3 specularAngle = normalize(normalVector * diffuse * 2.0 - lightDirection);  
    //vec3 specularColor = vec3(pow(clamp(dot(specularAngle, vertexToEye), 0.0, 1.0), u_specularExponent)); 

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
    #if defined(BUMPED)
    
    vec3 normalMapData = texture(u_normalmapTexture, v_texCoord).rgb;
    //float specularValue = normalMapData.b > 0.2 ? 1.0 : 0.0;
    float specularValue = clamp((normalMapData.b - 0.2) * 8.0, 0.0, 1.0);
    vec3 normalVector = normalize(vec3(normalMapData.rg * 2.0 - 1.0, 0.5));
    
    #else
    
    vec3 normalVector = normalize(v_normalVector);
    float specularValue = 1.0;
    
    #endif
    
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