use reqwest::Client;
use serde_json::{json, Value};

pub struct Bot {
    pub client: Client,
    pub base: String,
}

impl Bot {
    pub fn new(token: String) -> Self {
        let base = format!("https://api.telegram.org/bot{}", token);
        Bot { client: Client::new(), base }
    }

    pub async fn call(&self, method: &str, body: Value) -> Option<Value> {
        let url = format!("{}/{}", self.base, method);
        match self.client.post(url).json(&body).send().await {
            Ok(response) => match response.json::<Value>().await {
                Ok(value) => {
                    if value.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
                        Some(value)
                    } else {
                        eprintln!("Telegram {}: {}", method, value);
                        None
                    }
                }
                Err(error) => {
                    eprintln!("Telegram {} parse: {}", method, error);
                    None
                }
            },
            Err(error) => {
                eprintln!("Telegram {} network: {}", method, error);
                None
            }
        }
    }

    pub async fn get_updates(&self, offset: i64) -> Option<Value> {
        let params: Vec<(&str, String)> = vec![
            ("offset", offset.to_string()),
            ("timeout", "25".to_string()),
            ("allowed_updates", "[\"message\",\"callback_query\"]".to_string()),
        ];
        let url = format!("{}/getUpdates", self.base);
        match self.client.get(url).query(&params).send().await {
            Ok(response) => response.json::<Value>().await.ok(),
            Err(error) => {
                eprintln!("getUpdates network: {}", error);
                None
            }
        }
    }

    pub async fn send(&self, chat_id: i64, text: &str, markup: Option<Value>) {
        let mut body = json!({
            "chat_id": chat_id,
            "text": text,
            "parse_mode": "HTML",
        });
        if let Some(keyboard) = markup {
            body["reply_markup"] = keyboard;
        }
        self.call("sendMessage", body).await;
    }

    pub async fn edit(&self, chat_id: i64, message_id: i64, text: &str, markup: Option<Value>) {
        let mut body = json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "text": text,
            "parse_mode": "HTML",
        });
        if let Some(keyboard) = markup {
            body["reply_markup"] = keyboard;
        }
        self.call("editMessageText", body).await;
    }

    pub async fn answer_callback(&self, id: &str, text: Option<&str>, alert: bool) {
        let mut body = json!({"callback_query_id": id, "show_alert": alert});
        if let Some(value) = text {
            body["text"] = json!(value);
        }
        self.call("answerCallbackQuery", body).await;
    }
}

pub fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn keyboard(rows: Vec<Vec<(&str, String)>>) -> Value {
    let built: Vec<Vec<Value>> = rows
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|(text, data)| json!({"text": text, "callback_data": data}))
                .collect()
        })
        .collect();
    json!({"inline_keyboard": built})
}
