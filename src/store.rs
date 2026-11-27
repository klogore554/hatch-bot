use chrono::{FixedOffset, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

pub fn now() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}

pub fn msk() -> FixedOffset {
    FixedOffset::east_opt(3 * 3600).unwrap()
}

pub fn today() -> String {
    Utc::now().with_timezone(&msk()).format("%Y-%m-%d").to_string()
}

pub fn hhmm() -> String {
    Utc::now().with_timezone(&msk()).format("%H:%M").to_string()
}

#[derive(Serialize, Deserialize, Clone)]
pub struct User {
    pub username: String,
    pub source: String,
    pub referrer: Option<i64>,
    pub pro_until: f64,
    pub reminder_time: String,
    pub created_at: f64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Habit {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    pub created_at: f64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Log {
    pub user_id: i64,
    pub habit_id: i64,
    pub day: String,
    pub created_at: f64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Order {
    pub user_id: i64,
    pub days: i64,
    pub paid: bool,
    pub created_at: f64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Challenge {
    pub ends_at: f64,
    pub created_at: f64,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Member {
    pub name: String,
    pub done_today: bool,
    pub streak: i64,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Store {
    pub users: HashMap<i64, User>,
    pub habits: Vec<Habit>,
    pub logs: Vec<Log>,
    pub reminders: Vec<(i64, String)>,
    pub orders: HashMap<String, Order>,
    pub referrals: Vec<(i64, i64)>,
    pub challenges: HashMap<i64, Challenge>,
    pub members: HashMap<i64, HashMap<i64, Member>>,
    #[serde(skip)]
    pub path: PathBuf,
}

impl Store {
    pub fn load(path: PathBuf) -> Self {
        if let Ok(text) = fs::read_to_string(&path) {
            if let Ok(mut store) = serde_json::from_str::<Store>(&text) {
                store.path = path;
                return store;
            }
        }
        Store { path, ..Default::default() }
    }

    pub fn save(&self) {
        if let Some(parent) = self.path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let tmp = self.path.with_extension("tmp");
        if let Ok(text) = serde_json::to_string_pretty(self) {
            if fs::write(&tmp, text).is_ok() {
                let _ = fs::rename(&tmp, &self.path);
            }
        }
    }

    pub fn add_user(&mut self, id: i64, username: &str, source: &str) {
        self.users.entry(id).or_insert_with(|| User {
            username: username.to_string(),
            source: source.to_string(),
            referrer: None,
            pro_until: 0.0,
            reminder_time: "09:00".to_string(),
            created_at: now(),
        });
        self.save();
    }

    pub fn is_pro(&self, id: i64) -> bool {
        self.users.get(&id).map(|u| u.pro_until > now()).unwrap_or(false)
    }

    pub fn pro_until(&self, id: i64) -> f64 {
        self.users.get(&id).map(|u| u.pro_until).unwrap_or(0.0)
    }

    pub fn add_pro_days(&mut self, id: i64, days: i64) {
        if let Some(user) = self.users.get_mut(&id) {
            let base = user.pro_until.max(now());
            user.pro_until = base + days as f64 * 86400.0;
            self.save();
        }
    }

    fn habit_count(&self, id: i64) -> usize {
        self.habits.iter().filter(|h| h.user_id == id).count()
    }

    pub fn add_habit(&mut self, id: i64, name: &str) -> bool {
        if !self.is_pro(id) && self.habit_count(id) >= 1 {
            return false;
        }
        let next = self.habits.iter().map(|h| h.id).max().unwrap_or(0) + 1;
        self.habits.push(Habit {
            id: next,
            user_id: id,
            name: name.to_string(),
            created_at: now(),
        });
        self.save();
        true
    }

    pub fn list_habits(&self, id: i64) -> Vec<Habit> {
        let mut out: Vec<Habit> = self.habits.iter().filter(|h| h.user_id == id).cloned().collect();
        out.sort_by_key(|h| h.id);
        out
    }

    pub fn delete_habit(&mut self, id: i64, habit_id: i64) {
        self.habits.retain(|h| !(h.user_id == id && h.id == habit_id));
        self.logs.retain(|l| !(l.user_id == id && l.habit_id == habit_id));
        self.save();
    }

    pub fn mark_done(&mut self, id: i64, habit_id: i64) -> bool {
        let day = today();
        if self.logs.iter().any(|l| l.user_id == id && l.habit_id == habit_id && l.day == day) {
            return false;
        }
        self.logs.push(Log { user_id: id, habit_id, day, created_at: now() });
        self.save();
        true
    }

    pub fn streak(&self, id: i64, habit_id: i64) -> i64 {
        let days: Vec<String> = self
            .logs
            .iter()
            .filter(|l| l.user_id == id && l.habit_id == habit_id)
            .map(|l| l.day.clone())
            .collect();
        let mut cursor = Utc::now().with_timezone(&msk()).date_naive();
        if !days.iter().any(|d| d == &cursor.format("%Y-%m-%d").to_string()) {
            cursor = cursor.pred_opt().unwrap_or(cursor);
        }
        let mut count = 0;
        loop {
            let key = cursor.format("%Y-%m-%d").to_string();
            if !days.iter().any(|d| d == &key) {
                break;
            }
            count += 1;
            cursor = cursor.pred_opt().unwrap_or(cursor);
        }
        count
    }

    pub fn set_reminder(&mut self, id: i64, value: &str) {
        if let Some(user) = self.users.get_mut(&id) {
            user.reminder_time = value.to_string();
            self.save();
        }
    }

    pub fn users_to_remind(&self, time: &str, day: &str) -> Vec<i64> {
        self.users
            .iter()
            .filter(|(id, user)| {
                user.reminder_time == time
                    && !self.reminders.iter().any(|(u, d)| u == *id && d == day)
                    && !self.logs.iter().any(|l| l.user_id == **id && l.day == day)
            })
            .map(|(id, _)| *id)
            .collect()
    }

    pub fn mark_reminded(&mut self, id: i64, day: &str) {
        if !self.reminders.iter().any(|(u, d)| *u == id && d == day) {
            self.reminders.push((id, day.to_string()));
            self.save();
        }
    }

    pub fn was_reminded(&self, id: i64, day: &str) -> bool {
        self.reminders.iter().any(|(u, d)| *u == id && d == day)
    }

    pub fn add_order(&mut self, order_id: &str, user_id: i64) {
        self.orders.insert(
            order_id.to_string(),
            Order { user_id, days: 30, paid: false, created_at: now() },
        );
        self.save();
    }

    pub fn unpaid_orders(&self) -> Vec<(String, i64)> {
        self.orders
            .iter()
            .filter(|(_, o)| !o.paid)
            .map(|(id, o)| (id.clone(), o.user_id))
            .collect()
    }

    pub fn pay_order(&mut self, order_id: &str) -> Option<i64> {
        let order = self.orders.get(order_id)?.clone();
        if order.paid {
            return None;
        }
        if let Some(current) = self.orders.get_mut(order_id) {
            current.paid = true;
        }
        self.add_pro_days(order.user_id, order.days);
        self.save();
        Some(order.user_id)
    }

    pub fn add_referral(&mut self, referrer: i64, referred: i64) -> bool {
        if referrer == referred || self.referrals.iter().any(|(a, b)| *a == referrer && *b == referred) {
            return false;
        }
        self.referrals.push((referrer, referred));
        self.add_pro_days(referrer, 3);
        self.add_pro_days(referred, 3);
        true
    }

    pub fn create_challenge(&mut self, chat_id: i64, days: i64) -> f64 {
        let ends_at = now() + days as f64 * 86400.0;
        self.challenges.insert(
            chat_id,
            Challenge { ends_at, created_at: now() },
        );
        self.save();
        ends_at
    }

    pub fn challenge_active(&self, chat_id: i64) -> bool {
        self.challenges.get(&chat_id).map(|c| c.ends_at > now()).unwrap_or(false)
    }

    pub fn add_challenge_member(&mut self, chat_id: i64, user_id: i64, name: &str) {
        let members = self.members.entry(chat_id).or_default();
        members.entry(user_id).or_insert_with(|| Member {
            name: name.to_string(),
            done_today: false,
            streak: 0,
        });
        self.save();
    }

    pub fn challenge_done(&mut self, chat_id: i64, user_id: i64, name: &str) -> bool {
        self.add_challenge_member(chat_id, user_id, name);
        let member = self.members.get_mut(&chat_id).and_then(|m| m.get_mut(&user_id)).unwrap();
        if member.done_today {
            return false;
        }
        member.done_today = true;
        member.streak += 1;
        self.save();
        true
    }

    pub fn challenge_top(&self, chat_id: i64) -> Vec<(String, bool, i64)> {
        let mut rows: Vec<(String, bool, i64)> = self
            .members
            .get(&chat_id)
            .map(|m| {
                m.values()
                    .map(|v| (v.name.clone(), v.done_today, v.streak))
                    .collect()
            })
            .unwrap_or_default();
        rows.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)).then(a.0.cmp(&b.0)));
        rows.truncate(15);
        rows
    }

    pub fn reset_challenge_days(&mut self) {
        for members in self.members.values_mut() {
            for member in members.values_mut() {
                member.done_today = false;
            }
        }
        self.save();
    }

    pub fn expiring_pro_users(&self, hours: i64) -> Vec<i64> {
        let current = now();
        let limit = current + hours as f64 * 3600.0;
        self.users
            .iter()
            .filter(|(_, u)| u.pro_until > current && u.pro_until <= limit)
            .map(|(id, _)| *id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::Store;

    fn store() -> Store {
        let path = std::env::temp_dir().join(format!("habit-test-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        Store::load(path)
    }

    #[test]
    fn habits_referrals_and_orders() {
        let mut s = store();
        s.add_user(1, "one", "vid1");
        s.add_user(2, "two", "ref1");
        assert!(s.add_habit(1, "run"));
        assert!(!s.add_habit(1, "water"));
        assert!(s.mark_done(1, 1));
        assert!(!s.mark_done(1, 1));
        assert!(s.add_referral(1, 2));
        assert!(!s.add_referral(1, 2));
        assert!(s.is_pro(1));
        assert!(s.is_pro(2));
        s.add_order("o1", 1);
        assert_eq!(s.pay_order("o1"), Some(1));
        assert_eq!(s.pay_order("o1"), None);
        let _ = std::fs::remove_file(&s.path);
    }

    #[test]
    fn challenge_flow() {
        let mut s = store();
        s.create_challenge(-100, 7);
        assert!(s.challenge_active(-100));
        assert!(s.challenge_done(-100, 1, "A"));
        assert!(!s.challenge_done(-100, 1, "A"));
        assert_eq!(s.challenge_top(-100).len(), 1);
        s.reset_challenge_days();
        assert!(s.challenge_done(-100, 1, "A"));
        assert_eq!(s.challenge_top(-100)[0].2, 2);
        let _ = std::fs::remove_file(&s.path);
    }
}
