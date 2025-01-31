use crate::prelude::*;
use alcoholic_jwt::{validate, Validation, JWK};
use log::error;
use reqwest::Client;
use serde_json::Value;

const TOKEN_AUTH_INFO_URL: &str =
    "https://login.eveonline.com/.well-known/oauth-authorization-server";

/// Get the URL that hosts the valid JWT signing keys.
async fn get_keys_url(client: &Client) -> EsiResult<String> {
    let resp = client.get(TOKEN_AUTH_INFO_URL).send().await?;
    if resp.status() != 200 {
        error!(
            "Got status {} when making call to authenticate",
            resp.status()
        );
        return Err(EsiError::InvalidStatusCode(resp.status().as_u16()));
    }
    let data: Value = resp.json().await?;
    let url = data["jwks_uri"]
        .as_str()
        .ok_or_else(|| EsiError::InvalidJWT(String::from("Could not get keys URL")))?;
    Ok(url.to_owned())
}

/// Get the RS256 key to use.
async fn get_rs256_key(client: &Client) -> EsiResult<String> {
    let keys_url = get_keys_url(client).await?;
    let resp = client.get(&keys_url).send().await?;
    let data: Value = resp.json().await?;
    let key = data["keys"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["alg"].as_str().unwrap() == "RS256")
        .map(|entry| serde_json::to_string(entry).unwrap())
        .next()
        .ok_or_else(|| EsiError::InvalidJWT(String::from("Could not find an RS256 key")))?;
    Ok(key)
}

/// Decode and validate the SSO JWT, returning the contents.
pub(crate) async fn validate_jwt(
    client: &Client,
    token: &str,
    client_id: &str,
) -> EsiResult<TokenClaims> {
    let validation_key_str = get_rs256_key(client).await?;
    let validation_key: JWK = serde_json::from_str(&validation_key_str)?;
    let validations = vec![Validation::SubjectPresent, Validation::NotExpired];
    let token = validate(token, &validation_key, validations)?;
    /* Additional verifications from https://docs.esi.evetech.net/docs/sso/validating_eve_jwt.html */
    if token.claims["iss"].as_str().unwrap() != "login.eveonline.com"
        && token.claims["iss"].as_str().unwrap() != "https://login.eveonline.com"
    {
        return Err(EsiError::InvalidJWT(String::from(
            "JWT issuer is incorrect",
        )));
    }
    let claims = token.claims["aud"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect::<Vec<_>>();
    if claims.len() != 2 || !claims.contains(&"EVE Online") || !claims.contains(&client_id) {
        return Err(EsiError::InvalidJWT(String::from(
            "JWT audience is incorrect",
        )));
    }
    let token_claims = serde_json::from_value(token.claims)?;
    Ok(token_claims)
}
