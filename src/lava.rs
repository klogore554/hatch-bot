use hmac::{Hmac, Mac};
use reqwest::Client;
use serde_json::{json, Value};
use sha2::Sha256;
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};

type HmacSha256 = Hmac<Sha256>;

pub struct Lava {
    client: Client,
    shop_id: String,
    secret: String,
    product_id: String,
}

impl Lava {
    pub fn from_env() -> Self {
        Lava {
            client: Client::new(),
            shop_id: env::var("LAVA_SHOP_ID").unwrap_or_default(),
            secret: env::var("LAVA_SECRET_KEY").unwrap_or_default(),
            product_id: env::var("LAVA_PRODUCT_ID").unwrap_or_default(),
        }
    }

    pub fn configured(&self) -> bool {
        !self.shop_id.is_empty() && !self.secret.is_empty() && !self.product_id.is_empty()
    }

    async fn post(&self, path: &str, payload: Value) -> Value {
        let raw = serde_json::to_string(&payload).unwrap_or_default();
        let mut mac = HmacSha256::new_from_slice(self.secret.as_bytes()).unwrap();
        mac.update(raw.as_bytes());
        let signature = hex::encode(mac.finalize().into_bytes());

        let url = format!("https://api.lava.ru/business{}", path);
        match self
            .client
            .post(url)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("Signature", signature)
            .body(raw)
            .send()
            .await
        {
            Ok(response) => {
                let status = response.status();
                match response.json::<Value>().await {
                    Ok(value) => value,
                    Err(_) => json!({"error": format!("HTTP {}", status)}),
                }
            }
            Err(error) => json!({"error": error.to_string()}),
        }
    }

    pub async fn create_subscription(&self, user_id: i64, email: &str) -> (String, String, Value) {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let order_id = format!("tg-{}-{}", user_id, seconds);

        let consumer = self
            .post(
                "/recurrent/consumer/create",
                json!({
                    "shopId": self.shop_id,
                    "consumerId": user_id.to_string(),
                    "email": email,
                }),
            )
            .await;
        let consumer_id = consumer
            .get("data")
            .and_then(|d| d.get("consumerId"))
            .and_then(|v| v.as_str())
            .unwrap_or(&user_id.to_string())
            .to_string();

        let subscription = self
            .post(
                "/recurrent/subscription/subscribe",
                json!({
                    "shopId": self.shop_id,
                    "productId": self.product_id,
                    "consumerId": consumer_id,
                    "orderId": order_id,
                }),
            )
            .await;
        let url = subscription
            .get("data")
            .and_then(|d| d.get("url"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        (order_id, url, subscription)
    }

    pub async fn subscription_status(&self, order_id: &str) -> Value {
        self.post(
            "/recurrent/subscription/status",
            json!({"shopId": self.shop_id, "orderId": order_id}),
        )
        .await
    }

    pub fn looks_paid(value: &Value) -> bool {
        let status = value
            .get("data")
            .and_then(|d| d.get("status"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();
        let known = matches!(status.as_str(), "activated" | "success" | "paid" | "active");
        let ok = value.get("status_check").and_then(|v| v.as_bool()).unwrap_or(false)
            && value.get("status").and_then(|v| v.as_i64()).unwrap_or(0) == 200;
        known && (ok || value.get("data").is_some())
    }
}
