use anyhow::{anyhow, Error};
use serde::Deserialize;

use super::{AccessToken, ChargeState, BASE_URL};

#[derive(Deserialize, Debug)]
pub struct VehicleData {
    pub id: u64,
    pub vehicle_id: u64,
    pub vin: String,
    pub display_name: String,
    pub option_codes: String,
    pub color: Option<String>,
    pub access_type: String,
    pub tokens: Vec<String>,
    pub state: String,
    pub in_service: bool,
    pub id_s: String,
    pub calendar_enabled: bool,
    pub api_version: u64,
    pub command_signing: String,
}

impl VehicleData {
    fn is_online(&self) -> bool {
        self.state == "online"
    }
}

#[derive(Debug, Clone)]
pub struct Vehicle {
    pub id: u64,
    pub vehicle_id: u64,
    // this client has the token preconfigured
    // no handling of refreshes, so it will stop working after 45 days
    client: reqwest::Client,
}

impl AccessToken {
    #[tracing::instrument(skip(self))]
    pub async fn vehicles(&self, user_agent: &str) -> Result<Vec<Vehicle>, Error> {
        let client = self.build_client(user_agent);

        #[derive(Deserialize)]
        struct Response {
            response: Vec<VehicleData>,
        }

        let vehicles = client
            .get(format!("{}/api/1/vehicles", BASE_URL))
            .send()
            .await?
            .json::<Response>()
            .await?
            .response;

        Ok(vehicles
            .into_iter()
            .map(|VehicleData { id, vehicle_id, .. }| Vehicle {
                vehicle_id,
                id,
                client: client.clone(),
            })
            .collect())
    }
}

impl Vehicle {
    /// Wake the vehicle from sleep, returning only when the vehicle reports that it is online.
    #[tracing::instrument(skip(self))]
    pub async fn wake(&self) -> Result<(), Error> {
        #[derive(Deserialize)]
        struct Response {
            response: VehicleData,
        }

        use std::time::Duration;

        let waker = async {
            let mut wait = Duration::from_secs(1);
            loop {
                if self
                    .client
                    .post(format!("{}/api/1/vehicles/{}/wake_up", BASE_URL, self.id))
                    .send()
                    .await?
                    .json::<Response>()
                    .await?
                    .response
                    .is_online()
                {
                    tracing::debug!("vehicle is awake");
                    return Ok::<(), Error>(());
                } else {
                    tracing::debug!(?wait, "vehicle is asleep, waiting");
                    tokio::time::sleep(wait).await;
                    // exponential backoff
                    wait += wait;
                }
            }
        };

        tokio::time::timeout(Duration::from_secs(60), waker).await??;

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn data(&self) -> Result<VehicleData, Error> {
        #[derive(Deserialize)]
        struct Response {
            response: VehicleData,
        }

        Ok(self
            .client
            .get(format!("{}/api/1/vehicles/{}", BASE_URL, self.id))
            .send()
            .await?
            .json::<Response>()
            .await?
            .response)
    }

    #[tracing::instrument(skip(self))]
    pub async fn charge_state(&self) -> Result<ChargeState, Error> {
        #[derive(Deserialize)]
        struct Response {
            response: Option<ChargeState>,
        }

        Ok(self
            .client
            .get(format!(
                "{}/api/1/vehicles/{}/data_request/charge_state",
                BASE_URL, self.id
            ))
            .send()
            .await?
            .json::<Response>()
            .await?
            .response
            .ok_or_else(|| anyhow!("null ChargeState response"))?)
    }

    #[tracing::instrument(skip(self))]
    pub async fn charge_start(&self) -> Result<(), Error> {
        #[derive(Deserialize)]
        struct Response {
            response: ChargeResponse,
        }
        #[derive(Deserialize)]
        struct ChargeResponse {
            result: bool,
            reason: String,
        }

        let response = self
            .client
            .post(format!(
                "{}/api/1/vehicles/{}/command/charge_start",
                BASE_URL, self.id
            ))
            .send()
            .await?
            .json::<Response>()
            .await?
            .response;

        if response.result {
            Ok(())
        } else {
            Err(anyhow!("request failed, reason={}", response.reason))
        }
    }

    #[tracing::instrument(skip(self))]
    pub async fn charge_stop(&self) -> Result<(), Error> {
        #[derive(Deserialize)]
        struct Response {
            response: ChargeResponse,
        }
        #[derive(Deserialize)]
        struct ChargeResponse {
            result: bool,
            reason: String,
        }

        let response = self
            .client
            .post(format!(
                "{}/api/1/vehicles/{}/command/charge_stop",
                BASE_URL, self.id
            ))
            .send()
            .await?
            .json::<Response>()
            .await?
            .response;

        if response.result {
            Ok(())
        } else {
            Err(anyhow!("request failed, reason={}", response.reason))
        }
    }
}
