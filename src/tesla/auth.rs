use std::collections::HashMap;

use anyhow::{anyhow, Error};
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use scraper::{Html, Selector};
use serde::Deserialize;
use sha2::Digest;

static TESLA_CLIENT_ID: &str = "81527cff06843c8634fdc09e8ac0abefb46ac849f38fe1e431c2ef2106796384";
static TESLA_CLIENT_SECRET: &str =
    "c7257eb71a564034f9419ee651c7d0e5f7aa6bfbd18bafb5c5c033b093bb2fa3";

#[derive(Deserialize, Debug)]
pub struct AccessToken {
    pub access_token: String,
    pub created_at: i64,
    pub expires_in: i64,
    pub refresh_token: String,
    pub token_type: String,
}

impl AccessToken {
    pub async fn login(email: &str, password: &str, user_agent: &str) -> Result<Self, Error> {
        // “... if you want to make an API request to the Tesla servers, WHICH ARE IN HELL...”

        let state = String::from_utf8((0..32).map(|_| thread_rng().sample(Alphanumeric)).collect())
            .unwrap();

        let code_verifier = (0..86)
            .map(|_| thread_rng().sample(Alphanumeric))
            .collect::<Vec<u8>>();
        let code_challenge = base64::encode_config(
            hex::encode(sha2::Sha256::digest(&code_verifier)),
            base64::URL_SAFE,
        );

        let client = reqwest::Client::builder()
            .user_agent(user_agent)
            .redirect(reqwest::redirect::Policy::none())
            .build()?;

        tracing::debug!(?code_challenge, ?state, ?email);
        let rsp1 = client
            .get("https://auth.tesla.com/oauth2/v3/authorize")
            .query(&[
                ("client_id", "ownerapi"),
                ("code_challenge_method", "S256"),
                ("redirect_uri", "https://auth.tesla.com/void/callback"),
                ("response_type", "code"),
                ("scope", "openid email offline_access"),
            ])
            .query(&[
                ("code_challenge", code_challenge.as_str()),
                ("state", state.as_str()),
                ("login_hint", email),
            ])
            .send()
            .await?;

        let session_id_cookie = rsp1
            .headers()
            .get("set-cookie")
            .ok_or_else(|| anyhow!("missing cookie value"))?
            .to_owned();
        tracing::debug!(?session_id_cookie);

        let rsp1_body = rsp1.text().await?;
        tracing::debug!(?rsp1_body);
        let rsp1_html = Html::parse_document(&rsp1_body);

        let get_form_input = |name: &str| {
            let s = format!(r#"input[name="{}"]"#, name);
            let selector = Selector::parse(&s).unwrap();
            rsp1_html
                .select(&selector)
                .next()
                .and_then(|input| input.value().attr("value"))
                .ok_or_else(|| anyhow!("missing {} field", name))
        };

        let csrf = get_form_input("_csrf")?;
        let phase = get_form_input("_phase")?;
        let process = get_form_input("_process")?;
        let transaction_id = get_form_input("transaction_id")?;
        let cancel = get_form_input("cancel")?;

        tracing::debug!(?csrf, ?phase, ?process, ?transaction_id, ?cancel);

        let rsp2 = client
            .post("https://auth.tesla.com/oauth2/v3/authorize")
            .header("Cookie", session_id_cookie.clone())
            .query(&[
                ("client_id", "ownerapi"),
                ("code_challenge_method", "S256"),
                ("redirect_uri", "https://auth.tesla.com/void/callback"),
                ("response_type", "code"),
                ("scope", "openid email offline_access"),
            ])
            .query(&[
                ("code_challenge", code_challenge.as_str()),
                ("state", state.as_str()),
            ])
            .form(&[
                ("_csrf", csrf),
                ("_phase", phase),
                ("_process", process),
                ("transaction_id", transaction_id),
                ("cancel", cancel),
                ("identity", email),
                ("credential", password),
            ])
            .send()
            .await?;

        tracing::debug!(?rsp2);

        let location = rsp2
            .headers()
            .get("Location")
            .ok_or_else(|| anyhow!("missing Location header"))?;
        let location = reqwest::Url::parse(location.to_str()?)?;

        let (_, code) = location
            .query_pairs()
            .find(|(key, _)| key == "code")
            .ok_or_else(|| anyhow!("missing code value in redirect URL"))?;

        tracing::debug!(?location, ?code);

        let mut data = HashMap::<&str, &str>::default();
        data.insert("grant_type", "authorization_code");
        data.insert("client_id", "ownerapi");
        data.insert("code", code.as_ref());
        data.insert(
            "code_verifier",
            std::str::from_utf8(&code_verifier).unwrap(),
        );
        data.insert("redirect_uri", "https://auth.tesla.com/void/callback");

        let rsp3 = client
            .post("https://auth.tesla.com/oauth2/v3/token")
            .header("Cookie", session_id_cookie)
            .json(&data)
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        tracing::debug!(?rsp3);

        let sso_access_token = rsp3
            .get("access_token")
            .and_then(|val| val.as_str())
            .ok_or_else(|| anyhow!("missing access token"))?;

        let mut data = HashMap::<&str, &str>::default();
        data.insert("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer");
        data.insert("client_id", TESLA_CLIENT_ID);
        data.insert("client_secret", TESLA_CLIENT_SECRET);
        let token = client
            .post("https://owner-api.teslamotors.com/oauth/token")
            .header("Authorization", format!("Bearer {}", sso_access_token))
            .json(&data)
            .send()
            .await?
            .json::<AccessToken>()
            .await?;

        tracing::debug!(?token);

        Ok(token)
    }

    /// Build a [`reqwest::Client`] that supplies this token in request headers.
    pub(crate) fn build_client(&self, user_agent: &str) -> reqwest::Client {
        use reqwest::header;
        let mut headers = header::HeaderMap::new();
        let mut auth_value =
            header::HeaderValue::from_str(&format!("Bearer {}", self.access_token)).unwrap();
        auth_value.set_sensitive(true);
        headers.insert(header::AUTHORIZATION, auth_value);

        reqwest::Client::builder()
            .user_agent(user_agent)
            .default_headers(headers)
            .build()
            .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_creds() -> (String, String) {
        (
            std::env::var("TESLA_TEST_USER").expect("TESLA_TEST_USER is unset"),
            std::env::var("TESLA_TEST_PASS").expect("TESLA_TEST_PASS is unset"),
        )
    }

    #[tokio::test]
    async fn test_login() {
        tracing_subscriber::fmt::init();

        let (user, pass) = env_creds();

        let _token = AccessToken::login(&user, &pass, "sgip-ev-charging-test")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn tesla_scratch() {
        tracing_subscriber::fmt::init();

        let (user, pass) = env_creds();

        let token = AccessToken::login(&user, &pass, "sgip-ev-charging-test")
            .await
            .unwrap();

        let client = token.build_client("sgip-ev-charging-test");

        let base = "https://owner-api.teslamotors.com/";

        let vehicles = token.vehicles("sgip-ev-charging-test").await.unwrap();

        for v in &vehicles {
            let data = v.data().await.unwrap();
            tracing::info!(?v, ?data);
        }

        let v = vehicles[0].clone();
        let id = v.id;

        v.wake().await.unwrap();
        let charge_state = v.charge_state().await.unwrap();
        tracing::info!(?charge_state);
        tracing::info!(rsp = ?v.charge_start().await);
        tracing::info!(rsp = ?v.charge_start().await);
        let charge_state = v.charge_state().await.unwrap();
        tracing::info!(?charge_state);
        tracing::info!(rsp = ?v.charge_stop().await);
        tracing::info!(rsp = ?v.charge_stop().await);

        todo!()
    }
}
