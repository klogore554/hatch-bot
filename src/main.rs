mod handlers;
mod lava;
mod legal;
mod store;
mod telegram;

use handlers::Ctx;
use lava::Lava;
use serde_json::json;
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use telegram::Bot;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    let token = env::var("BOT_TOKEN").expect("Не задан BOT_TOKEN");
    let bot = Arc::new(Bot::new(token));

    let me = bot.call("getMe", json!({})).await;
    let me_username = me
        .as_ref()
        .and_then(|v| v.get("result"))
        .and_then(|v| v.get("username"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let db_path = PathBuf::from(
        env::var("DB_PATH").unwrap_or_else(|_| "habit.json".to_string()),
    );

    let ctx = Arc::new(Ctx {
        bot: bot.clone(),
        store: Arc::new(Mutex::new(store::Store::load(db_path))),
        states: Arc::new(Mutex::new(HashMap::new())),
        lava: Lava::from_env(),
        me_username,
    });

    if env::var("HEALTH_SERVER").unwrap_or_else(|_| "1".to_string()) == "1" {
        tokio::spawn(health_server());
    }
    let scheduler_ctx = ctx.clone();
    tokio::spawn(scheduler(scheduler_ctx));

    eprintln!("Rust bot started");

    let mut offset = 0i64;
    loop {
        if let Some(value) = bot.get_updates(offset).await {
            if let Some(updates) = value.get("result").and_then(|v| v.as_array()) {
                for update in updates {
                    if let Some(id) = update["update_id"].as_i64() {
                        offset = id + 1;
                    }
                    let current = ctx.clone();
                    if let Some(message) = update.get("message") {
                        handlers::message(current, message).await;
                    } else if let Some(callback) = update.get("callback_query") {
                        handlers::callback(current, callback).await;
                    }
                }
            }
        }
    }
}

async fn scheduler(ctx: Arc<Ctx>) {
    let mut payments = tokio::time::interval(Duration::from_secs(60));
    let mut reminders = tokio::time::interval(Duration::from_secs(60));
    let mut expiring = tokio::time::interval(Duration::from_secs(6 * 3600));
    let mut last_reset_date = String::new();

    loop {
        tokio::select! {
            _ = payments.tick() => handlers::check_payments(&ctx).await,
            _ = reminders.tick() => {
                handlers::send_reminders(&ctx).await;
                let day = store::today();
                if store::hhmm() == "00:05" && last_reset_date != day {
                    last_reset_date = day;
                    ctx.store.lock().unwrap().reset_challenge_days();
                }
            }
            _ = expiring.tick() => handlers::warn_expiring(&ctx).await,
        }
    }
}

async fn health_server() {
    let port = env::var("PORT").unwrap_or_else(|_| "8000".to_string());
    let listener = match TcpListener::bind(format!("0.0.0.0:{}", port)).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("Health server: {}", error);
            return;
        }
    };
    loop {
        if let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buffer = [0u8; 1024];
                let _ = socket.read(&mut buffer).await;
                let body = r#"{"ok":true}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    }
}
