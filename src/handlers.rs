use crate::lava::Lava;
use crate::legal;
use crate::store::{self, Store};
use crate::telegram::{escape_html, keyboard, Bot};
use chrono::{NaiveTime, TimeZone, Utc};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub const PRESETS: [(&str, &str); 4] = [
    ("run", "🏃 Утренний бег"),
    ("water", "💧 Пить воду"),
    ("read", "📚 Читать 10 минут"),
    ("med", "🧘 Медитация"),
];

#[derive(Clone, Copy)]
pub enum Fsm {
    Email,
    HabitName,
}

pub struct Ctx {
    pub bot: Arc<Bot>,
    pub store: Arc<Mutex<Store>>,
    pub states: Arc<Mutex<HashMap<i64, Fsm>>>,
    pub lava: Lava,
    pub me_username: String,
}

fn main_keyboard() -> Value {
    keyboard(vec![
        vec![("➕ Привычка", "menu_add".into()), ("✅ Отметить", "menu_done".into())],
        vec![("📊 Стрик", "menu_streak".into()), ("👥 Челлендж", "menu_group".into())],
        vec![("💎 Pro", "menu_pro".into()), ("📄 Документы", "menu_docs".into())],
    ])
}

fn docs_keyboard() -> Value {
    keyboard(vec![
        vec![
            ("Оферта", "doc_offer".into()),
            ("Возврат", "doc_refund".into()),
            ("Данные", "doc_privacy".into()),
        ],
        vec![("« Назад", "menu_back".into())],
    ])
}

fn add_keyboard() -> Value {
    let mut rows: Vec<Vec<(&str, String)>> = Vec::new();
    for pair in PRESETS.chunks(2) {
        rows.push(pair.iter().map(|(k, v)| (*v, format!("add:{}", k))).collect());
    }
    rows.push(vec![("✍️ Своя", "add:custom".into())]);
    rows.push(vec![("« Назад", "menu_back".into())]);
    keyboard(rows)
}

fn habit_keyboard(store: &Store, user_id: i64) -> Value {
    let mut buttons: Vec<Value> = store
        .list_habits(user_id)
        .into_iter()
        .map(|h| json!({"text": format!("✅ {}", h.name), "callback_data": format!("done:{}", h.id)}))
        .collect();
    if store.is_pro(user_id) {
        buttons.extend(store.list_habits(user_id).into_iter().map(|h| {
            json!({"text": format!("🗑 {}", h.name), "callback_data": format!("del:{}", h.id)})
        }));
    }
    buttons.push(json!({"text": "« Назад", "callback_data": "menu_back"}));
    let built: Vec<Vec<Value>> = buttons.into_iter().map(|b| vec![b]).collect();
    json!({"inline_keyboard": built})
}

fn challenge_keyboard() -> Value {
    keyboard(vec![vec![
        ("✅ Я сделал", "gdone".into()),
        ("🏆 Топ", "gtop".into()),
    ]])
}

fn pro_keyboard() -> Value {
    keyboard(vec![vec![("💳 Оформить", "go_sub".into())]])
}

pub async fn show_menu(ctx: &Ctx, chat_id: i64, user_id: i64) {
    let pro = ctx.store.lock().unwrap().is_pro(user_id);
    let mark = if pro { "💎 Pro" } else { "🆓 Free" };
    let text = format!(
        "<b>Хаб-бот</b> · {}\n\nОтмечай привычки, собирай стрики.\nБесплатно — 1 привычка. Про — без лимита и со статистикой.\n\nПриведи 2 друзей — получи 3 дня Pro.",
        mark
    );
    ctx.bot.send(chat_id, &text, Some(main_keyboard())).await;
}

fn set_state(ctx: &Ctx, user_id: i64, state: Fsm) {
    ctx.states.lock().unwrap().insert(user_id, state);
}

fn clear_state(ctx: &Ctx, user_id: i64) {
    ctx.states.lock().unwrap().remove(&user_id);
}

pub async fn message(ctx: Arc<Ctx>, msg: &Value) {
    let Some(user_id) = msg["from"]["id"].as_i64() else { return };
    let Some(chat_id) = msg["chat"]["id"].as_i64() else { return };
    let text = msg["text"].as_str().unwrap_or("");
    let username = msg["from"]["username"].as_str().unwrap_or("");

    let current = ctx.states.lock().unwrap().get(&user_id).copied();
    if let Some(state) = current {
        if text.starts_with("/cancel") {
            clear_state(&ctx, user_id);
            ctx.bot.send(chat_id, "Отменил.", None).await;
            return;
        }
        match state {
            Fsm::Email => {
                let email = text.trim();
                if !email.contains('@') {
                    ctx.bot.send(chat_id, "Похоже, это не email. Попробуй ещё или /cancel", None).await;
                    return;
                }
                if !ctx.lava.configured() {
                    ctx.bot.send(chat_id, "Оплата ещё не настроена. Напиши в поддержку бота.", None).await;
                    clear_state(&ctx, user_id);
                    return;
                }
                let (order_id, url, raw) = ctx.lava.create_subscription(user_id, email).await;
                if url.is_empty() {
                    ctx.bot.send(chat_id, "Lava не вернула ссылку на оплату. Проверь настройки продукта.", None).await;
                    eprintln!("Ответ Lava без url: {}", raw);
                } else {
                    ctx.store.lock().unwrap().add_order(&order_id, user_id);
                    ctx.bot
                        .send(
                            chat_id,
                            &format!("Ссылка на оплату:\n{}\n\nПосле оплаты доступ продлится сам — проверяю раз в минуту.", url),
                            None,
                        )
                        .await;
                }
                clear_state(&ctx, user_id);
                return;
            }
            Fsm::HabitName => {
                let name = text.trim().chars().take(40).collect::<String>();
                if name.is_empty() {
                    ctx.bot.send(chat_id, "Пусто. Напиши ещё раз или /cancel", None).await;
                    return;
                }
                let ok = ctx.store.lock().unwrap().add_habit(user_id, &name);
                let answer = if ok {
                    format!("Добавил: {}", escape_html(&name))
                } else {
                    "Бесплатно только 1 привычка. /subscribe".to_string()
                };
                ctx.bot.send(chat_id, &answer, None).await;
                clear_state(&ctx, user_id);
                return;
            }
        }
    }

    let source = text.splitn(2, ' ').nth(1).unwrap_or("").to_string();
    let mut parts = text.split_whitespace();
    let command = parts.next().unwrap_or("").to_lowercase();
    let arg = parts.next().unwrap_or("").to_string();

    match command.as_str() {
        "/start" => {
            ctx.store.lock().unwrap().add_user(user_id, username, &source);
            if source.starts_with("ref") && source[3..].chars().all(|c| c.is_ascii_digit()) && source.len() > 3 {
                if let Ok(referrer) = source[3..].parse::<i64>() {
                    if ctx.store.lock().unwrap().add_referral(referrer, user_id) {
                        ctx.bot
                            .send(
                                referrer,
                                "🔥 Кто-то запустил бота по твоей ссылке. +3 дня Pro уже начислены.",
                                None,
                            )
                            .await;
                    }
                    ctx.bot.send(chat_id, "🎁 Ты пришёл по ссылке друга — +3 дня Pro!", None).await;
                }
            }
            clear_state(&ctx, user_id);
            show_menu(&ctx, chat_id, user_id).await;
        }
        "/help" => {
            ctx.bot
                .send(
                    chat_id,
                    "/start — меню\n/add — добавить привычку\n/today — отметить сегодня\n/streak — серии\n/remind 09:00 — время напоминания\n/referral — твоя ссылка\n/subscribe — подписка Pro\n/challenge — челлендж в группе\n/top — рейтинг группы",
                    None,
                )
                .await;
        }
        "/menu" => show_menu(&ctx, chat_id, user_id).await,
        "/add" => ctx.bot.send(chat_id, "Выбери привычку:", Some(add_keyboard())).await,
        "/today" => {
            let habits = ctx.store.lock().unwrap().list_habits(user_id);
            if habits.is_empty() {
                ctx.bot.send(chat_id, "Сначала добавь привычку: /add", None).await;
            } else {
                ctx.bot.send(chat_id, "Что сделал сегодня?", Some(habit_keyboard(&ctx.store.lock().unwrap(), user_id))).await;
            }
        }
        "/streak" => {
            let store = ctx.store.lock().unwrap();
            let habits = store.list_habits(user_id);
            let text = if habits.is_empty() {
                "Привычек пока нет — /add".to_string()
            } else {
                let lines: Vec<String> = habits
                    .iter()
                    .map(|h| format!("{}: <b>{}</b> дн.", escape_html(&h.name), store.streak(user_id, h.id)))
                    .collect();
                format!("Текущие серии:\n{}", lines.join("\n"))
            };
            drop(store);
            ctx.bot.send(chat_id, &text, None).await;
        }
        "/remind" => {
            if NaiveTime::parse_from_str(&arg, "%H:%M").is_err() {
                ctx.bot.send(chat_id, "Пример: /remind 09:00", None).await;
            } else {
                ctx.store.lock().unwrap().set_reminder(user_id, &arg);
                ctx.bot.send(chat_id, &format!("Напоминание каждый день в {} (МСК).", arg), None).await;
            }
        }
        "/referral" => {
            let link = format!("https://t.me/{}?start=ref{}", ctx.me_username, user_id);
            ctx.bot
                .send(
                    chat_id,
                    &format!("Твоя ссылка:\n<code>{}</code>\n\nЗа каждого друга, который запустит бота, ты и он получаете +3 дня Pro.", link),
                    None,
                )
                .await;
        }
        "/pro" => send_pro(&ctx, chat_id, user_id).await,
        "/subscribe" => {
            set_state(&ctx, user_id, Fsm::Email);
            ctx.bot.send(chat_id, "Напиши email, который указан в «Моём налоге» — туда пойдёт чек:", None).await;
        }
        "/cancel" => {
            clear_state(&ctx, user_id);
            ctx.bot.send(chat_id, "Отменил.", None).await;
        }
        "/challenge" => {
            if chat_id > 0 {
                ctx.bot.send(chat_id, "Команду /challenge надо написать прямо в группе.", None).await;
                return;
            }
            let ends = ctx.store.lock().unwrap().create_challenge(chat_id, 7);
            ctx.store
                .lock()
                .unwrap()
                .add_challenge_member(chat_id, user_id, msg["from"]["first_name"].as_str().unwrap_or(""));
            let until = Utc
                .timestamp_opt(ends as i64, 0)
                .single()
                .unwrap_or_else(Utc::now)
                .with_timezone(&store::msk())
                .format("%d.%m.%Y");
            ctx.bot
                .send(
                    chat_id,
                    &format!("🏁 Челлендж на 7 дней запущен!\nДо {}.\n\nКаждый день жми «Я сделал». В конце недели покажу победителей.\nЗови друзей — им тоже захочется быть в рейтинге.", until),
                    Some(challenge_keyboard()),
                )
                .await;
        }
        "/top" => send_top(&ctx, chat_id).await,
        _ => {}
    }
}

async fn send_pro(ctx: &Ctx, chat_id: i64, user_id: i64) {
    let until = ctx.store.lock().unwrap().pro_until(user_id);
    let status = if until > store::now() {
        Utc.timestamp_opt(until as i64, 0)
            .single()
            .unwrap_or_else(Utc::now)
            .with_timezone(&store::msk())
            .format("%d.%m.%Y")
            .to_string()
    } else {
        "не активен".to_string()
    };
    ctx.bot
        .send(
            chat_id,
            &format!("<b>Хаб-бот Pro</b> — 299 ₽/мес\n\n• безлимитные привычки\n• полная статистика и серии\n• гибкое время напоминаний\n• +3 дня за каждого друга\n\nТвой Pro: {}", status),
            Some(pro_keyboard()),
        )
        .await;
}

async fn send_top(ctx: &Ctx, chat_id: i64) {
    let rows = ctx.store.lock().unwrap().challenge_top(chat_id);
    let text = if rows.is_empty() {
        "В этом чате ещё нет челленджа.".to_string()
    } else {
        let lines: Vec<String> = rows
            .iter()
            .enumerate()
            .map(|(i, (name, done, streak))| {
                format!(
                    "{}. {} {} — {} дн.",
                    i + 1,
                    if *done { "✅" } else { "⬜" },
                    escape_html(name),
                    streak
                )
            })
            .collect();
        format!("🏆 Рейтинг:\n{}", lines.join("\n"))
    };
    ctx.bot.send(chat_id, &text, None).await;
}

pub async fn callback(ctx: Arc<Ctx>, cb: &Value) {
    let Some(user_id) = cb["from"]["id"].as_i64() else { return };
    let Some(chat_id) = cb["message"]["chat"]["id"].as_i64() else { return };
    let Some(message_id) = cb["message"]["message_id"].as_i64() else { return };
    let Some(data) = cb["data"].as_str() else { return };
    let callback_id = cb["id"].as_str().unwrap_or("");

    if let Some(document) = legal::get(data) {
        if cb["message"]["text"].as_str() != Some(document.as_str()) {
            ctx.bot.edit(chat_id, message_id, &document, Some(docs_keyboard())).await;
        }
        ctx.bot.answer_callback(callback_id, None, false).await;
        return;
    }

    match data {
        "menu_back" => {
            show_menu(&ctx, chat_id, user_id).await;
            ctx.bot.answer_callback(callback_id, None, false).await;
        }
        "noop" => ctx.bot.answer_callback(callback_id, None, false).await,
        "menu_docs" => {
            ctx.bot.edit(chat_id, message_id, "Документы:", Some(docs_keyboard())).await;
            ctx.bot.answer_callback(callback_id, None, false).await;
        }
        "menu_add" => {
            ctx.bot.send(chat_id, "Выбери привычку:", Some(add_keyboard())).await;
            ctx.bot.answer_callback(callback_id, None, false).await;
        }
        "menu_done" | "menu_streak" | "menu_pro" => {
            if data == "menu_done" {
                let habits = ctx.store.lock().unwrap().list_habits(user_id);
                if habits.is_empty() {
                    ctx.bot.send(chat_id, "Сначала добавь привычку: /add", None).await;
                } else {
                    let markup = habit_keyboard(&ctx.store.lock().unwrap(), user_id);
                    ctx.bot.send(chat_id, "Что сделал сегодня?", Some(markup)).await;
                }
            } else if data == "menu_streak" {
                let store = ctx.store.lock().unwrap();
                let habits = store.list_habits(user_id);
                let text = if habits.is_empty() {
                    "Привычек пока нет — /add".to_string()
                } else {
                    let lines: Vec<String> = habits
                        .iter()
                        .map(|h| format!("{}: <b>{}</b> дн.", escape_html(&h.name), store.streak(user_id, h.id)))
                        .collect();
                    format!("Текущие серии:\n{}", lines.join("\n"))
                };
                drop(store);
                ctx.bot.send(chat_id, &text, None).await;
            } else {
                send_pro(&ctx, chat_id, user_id).await;
            }
            ctx.bot.answer_callback(callback_id, None, false).await;
        }
        "menu_group" => {
            ctx.bot
                .send(chat_id, "Добавь бота в группу и напиши там /challenge.\nВ группе будет рейтинг, а участники сами будут звать друзей.", None)
                .await;
            ctx.bot.answer_callback(callback_id, None, false).await;
        }
        "go_sub" => {
            set_state(&ctx, user_id, Fsm::Email);
            ctx.bot.send(chat_id, "Напиши email, который указан в «Моём налоге» — туда пойдёт чек:", None).await;
            ctx.bot.answer_callback(callback_id, None, false).await;
        }
        "gdone" => {
            let active = ctx.store.lock().unwrap().challenge_active(chat_id);
            if !active {
                ctx.bot.answer_callback(callback_id, Some("Челлендж закончился. Напиши /challenge"), true).await;
                return;
            }
            let name = cb["from"]["first_name"].as_str().unwrap_or("").to_string();
            let fresh = ctx.store.lock().unwrap().challenge_done(chat_id, user_id, &name);
            ctx.bot.answer_callback(callback_id, Some(if fresh { "Отметил ✅" } else { "Сегодня уже отмечен" }), false).await;
        }
        "gtop" => {
            send_top(&ctx, chat_id).await;
            ctx.bot.answer_callback(callback_id, None, false).await;
        }
        _ if data.starts_with("add:") => {
            let key = &data[4..];
            if key == "custom" {
                set_state(&ctx, user_id, Fsm::HabitName);
                ctx.bot.send(chat_id, "Напиши название привычки (до 40 символов):", None).await;
                ctx.bot.answer_callback(callback_id, None, false).await;
                return;
            }
            let name = PRESETS.iter().find(|(k, _)| *k == key).map(|(_, v)| *v);
            let Some(name) = name else {
                ctx.bot.answer_callback(callback_id, Some("Не знаю такую привычку"), true).await;
                return;
            };
            let ok = ctx.store.lock().unwrap().add_habit(user_id, name);
            ctx.bot.answer_callback(callback_id, Some(if ok { "Добавлено ✅" } else { "Лимит" }), !ok).await;
            if ok {
                ctx.bot.edit(chat_id, message_id, &format!("Добавил: {}", name), None).await;
            } else {
                ctx.bot.send(chat_id, "Бесплатно можно вести 1 привычку. Оформи /subscribe — и без лимита.", None).await;
            }
        }
        _ if data.starts_with("done:") => {
            let Ok(habit_id) = data[5..].parse::<i64>() else { return };
            let fresh = ctx.store.lock().unwrap().mark_done(user_id, habit_id);
            if fresh {
                let streak = ctx.store.lock().unwrap().streak(user_id, habit_id);
                ctx.bot.edit(chat_id, message_id, &format!("🔥 Отмечено! Стрик: {} дн.", streak), None).await;
                ctx.bot.answer_callback(callback_id, Some("Красавчик"), false).await;
            } else {
                ctx.bot.answer_callback(callback_id, Some("Уже отмечено сегодня"), true).await;
            }
        }
        _ if data.starts_with("del:") => {
            let Ok(habit_id) = data[4..].parse::<i64>() else { return };
            ctx.store.lock().unwrap().delete_habit(user_id, habit_id);
            ctx.bot.edit(chat_id, message_id, "Удалил.", None).await;
            ctx.bot.answer_callback(callback_id, None, false).await;
        }
        _ => ctx.bot.answer_callback(callback_id, None, false).await,
    }
}

pub async fn check_payments(ctx: &Arc<Ctx>) {
    if !ctx.lava.configured() {
        return;
    }
    let orders = ctx.store.lock().unwrap().unpaid_orders();
    for (order_id, user_id) in orders {
        let result = ctx.lava.subscription_status(&order_id).await;
        if Lava::looks_paid(&result) {
            if ctx.store.lock().unwrap().pay_order(&order_id).is_some() {
                ctx.bot.send(user_id, "💎 Оплата прошла! Pro продлён на 30 дней. Спасибо!", None).await;
            }
        }
    }
}

pub async fn send_reminders(ctx: &Arc<Ctx>) {
    let time = store::hhmm();
    let day = store::today();
    let users = ctx.store.lock().unwrap().users_to_remind(&time, &day);
    for user_id in users {
        ctx.store.lock().unwrap().mark_reminded(user_id, &day);
        let markup = keyboard(vec![vec![("✅ Открыть", "menu_done".into())]]);
        ctx.bot
            .send(user_id, "Сегодня тренировка? Не забудь отметить привычку ✅", Some(markup))
            .await;
    }
}

pub async fn warn_expiring(ctx: &Arc<Ctx>) {
    let marker = format!("warn-{}", store::today());
    let users = ctx.store.lock().unwrap().expiring_pro_users(72);
    for user_id in users {
        if ctx.store.lock().unwrap().was_reminded(user_id, &marker) {
            continue;
        }
        ctx.store.lock().unwrap().mark_reminded(user_id, &marker);
        ctx.bot.send(user_id, "Pro заканчивается через 3 дня. Продлить можно в /subscribe.", None).await;
    }
}
