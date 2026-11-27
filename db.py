import os
import sqlite3
import time
from datetime import datetime, timedelta, timezone
from pathlib import Path

DB_PATH = Path(os.getenv("DB_PATH") or Path(__file__).with_name("habit.db"))
DB_PATH.parent.mkdir(parents=True, exist_ok=True)
conn = sqlite3.connect(DB_PATH, check_same_thread=False)
conn.row_factory = sqlite3.Row

MSK = timezone(timedelta(hours=3))


def now_msk() -> datetime:
    return datetime.now(MSK)


def today() -> str:
    return now_msk().strftime("%Y-%m-%d")


def init_db() -> None:
    conn.executescript(
        """
        CREATE TABLE IF NOT EXISTS users (
            user_id INTEGER PRIMARY KEY,
            username TEXT,
            source TEXT DEFAULT '',
            referrer INTEGER,
            pro_until REAL DEFAULT 0,
            reminder_time TEXT DEFAULT '09:00',
            created_at REAL
        );

        CREATE TABLE IF NOT EXISTS habits (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id INTEGER NOT NULL,
            name TEXT NOT NULL,
            created_at REAL
        );

        CREATE TABLE IF NOT EXISTS logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id INTEGER NOT NULL,
            habit_id INTEGER NOT NULL,
            day TEXT NOT NULL,
            created_at REAL,
            UNIQUE(user_id, habit_id, day)
        );

        CREATE TABLE IF NOT EXISTS reminders (
            user_id INTEGER NOT NULL,
            day TEXT NOT NULL,
            created_at REAL,
            PRIMARY KEY(user_id, day)
        );

        CREATE TABLE IF NOT EXISTS orders (
            order_id TEXT PRIMARY KEY,
            user_id INTEGER NOT NULL,
            days INTEGER DEFAULT 30,
            paid INTEGER DEFAULT 0,
            created_at REAL
        );

        CREATE TABLE IF NOT EXISTS referrals (
            referrer INTEGER NOT NULL,
            referred INTEGER NOT NULL,
            created_at REAL,
            PRIMARY KEY(referrer, referred)
        );

        CREATE TABLE IF NOT EXISTS challenges (
            chat_id INTEGER PRIMARY KEY,
            ends_at REAL NOT NULL,
            created_at REAL
        );

        CREATE TABLE IF NOT EXISTS challenge_members (
            chat_id INTEGER NOT NULL,
            user_id INTEGER NOT NULL,
            name TEXT NOT NULL,
            done_today INTEGER DEFAULT 0,
            streak INTEGER DEFAULT 0,
            PRIMARY KEY(chat_id, user_id)
        );
        """
    )
    conn.commit()


def add_user(user_id: int, username: str | None, source: str = "") -> None:
    conn.execute(
        "INSERT OR IGNORE INTO users (user_id, username, source, created_at) VALUES (?, ?, ?, ?)",
        (user_id, username or "", source or "", time.time()),
    )
    conn.commit()


def get_user(user_id: int):
    return conn.execute("SELECT * FROM users WHERE user_id = ?", (user_id,)).fetchone()


def is_pro(user_id: int) -> bool:
    row = get_user(user_id)
    return bool(row and row["pro_until"] > time.time())


def pro_until(user_id: int) -> float:
    row = get_user(user_id)
    return row["pro_until"] if row else 0.0


def add_pro_days(user_id: int, days: int) -> None:
    current = max(pro_until(user_id), time.time())
    conn.execute(
        "UPDATE users SET pro_until = ? WHERE user_id = ?",
        (current + days * 86400, user_id),
    )
    conn.commit()


def habit_count(user_id: int) -> int:
    return conn.execute(
        "SELECT COUNT(*) AS c FROM habits WHERE user_id = ?", (user_id,)
    ).fetchone()["c"]


def add_habit(user_id: int, name: str) -> bool:
    if not is_pro(user_id) and habit_count(user_id) >= 1:
        return False
    conn.execute(
        "INSERT INTO habits (user_id, name, created_at) VALUES (?, ?, ?)",
        (user_id, name, time.time()),
    )
    conn.commit()
    return True


def list_habits(user_id: int):
    return conn.execute(
        "SELECT id, name FROM habits WHERE user_id = ? ORDER BY id", (user_id,)
    ).fetchall()


def delete_habit(user_id: int, habit_id: int) -> None:
    conn.execute("DELETE FROM habits WHERE id = ? AND user_id = ?", (habit_id, user_id))
    conn.execute("DELETE FROM logs WHERE habit_id = ? AND user_id = ?", (habit_id, user_id))
    conn.commit()


def mark_done(user_id: int, habit_id: int, day: str | None = None) -> bool:
    day = day or today()
    cur = conn.execute(
        "INSERT OR IGNORE INTO logs (user_id, habit_id, day, created_at) VALUES (?, ?, ?, ?)",
        (user_id, habit_id, day, time.time()),
    )
    conn.commit()
    return cur.rowcount > 0


def done_today(user_id: int) -> bool:
    return bool(
        conn.execute(
            "SELECT 1 FROM logs WHERE user_id = ? AND day = ? LIMIT 1",
            (user_id, today()),
        ).fetchone()
    )


def streak(user_id: int, habit_id: int) -> int:
    days = []
    rows = conn.execute(
        "SELECT day FROM logs WHERE user_id = ? AND habit_id = ? ORDER BY day DESC",
        (user_id, habit_id),
    ).fetchall()
    days = {r["day"] for r in rows}
    start = now_msk().date()
    if start.isoformat() not in days:
        start -= timedelta(days=1)
    count = 0
    while start.isoformat() in days:
        count += 1
        start -= timedelta(days=1)
    return count


def set_reminder(user_id: int, value: str) -> None:
    conn.execute("UPDATE users SET reminder_time = ? WHERE user_id = ?", (value, user_id))
    conn.commit()


def users_to_remind(hhmm: str, day: str):
    return conn.execute(
        """
        SELECT u.user_id FROM users u
        WHERE u.reminder_time = ?
          AND NOT EXISTS (SELECT 1 FROM reminders r WHERE r.user_id = u.user_id AND r.day = ?)
          AND NOT EXISTS (SELECT 1 FROM logs l WHERE l.user_id = u.user_id AND l.day = ?)
        """,
        (hhmm, day, day),
    ).fetchall()


def mark_reminded(user_id: int, day: str) -> None:
    conn.execute(
        "INSERT OR IGNORE INTO reminders (user_id, day, created_at) VALUES (?, ?, ?)",
        (user_id, day, time.time()),
    )
    conn.commit()


def was_reminded(user_id: int, day: str) -> bool:
    return bool(
        conn.execute(
            "SELECT 1 FROM reminders WHERE user_id = ? AND day = ?",
            (user_id, day),
        ).fetchone()
    )


def add_order(order_id: str, user_id: int, days: int = 30) -> None:
    conn.execute(
        "INSERT INTO orders (order_id, user_id, days, created_at) VALUES (?, ?, ?, ?)",
        (order_id, user_id, days, time.time()),
    )
    conn.commit()


def unpaid_orders():
    return conn.execute("SELECT * FROM orders WHERE paid = 0").fetchall()


def pay_order(order_id: str) -> int | None:
    row = conn.execute("SELECT * FROM orders WHERE order_id = ?", (order_id,)).fetchone()
    if not row or row["paid"]:
        return None
    conn.execute("UPDATE orders SET paid = 1 WHERE order_id = ?", (order_id,))
    add_pro_days(row["user_id"], row["days"])
    conn.commit()
    return row["user_id"]


def add_referral(referrer: int, referred: int) -> bool:
    if referrer == referred:
        return False
    cur = conn.execute(
        "INSERT OR IGNORE INTO referrals (referrer, referred, created_at) VALUES (?, ?, ?)",
        (referrer, referred, time.time()),
    )
    conn.commit()
    if cur.rowcount:
        add_pro_days(referrer, 3)
        add_pro_days(referred, 3)
        return True
    return False


def create_challenge(chat_id: int, days: int = 7) -> float:
    ends_at = time.time() + days * 86400
    conn.execute(
        "INSERT INTO challenges (chat_id, ends_at, created_at) VALUES (?, ?, ?) "
        "ON CONFLICT(chat_id) DO UPDATE SET ends_at = excluded.ends_at",
        (chat_id, ends_at, time.time()),
    )
    conn.commit()
    return ends_at


def challenge_active(chat_id: int) -> bool:
    row = conn.execute("SELECT ends_at FROM challenges WHERE chat_id = ?", (chat_id,)).fetchone()
    return bool(row and row["ends_at"] > time.time())


def add_challenge_member(chat_id: int, user_id: int, name: str) -> None:
    conn.execute(
        "INSERT OR IGNORE INTO challenge_members (chat_id, user_id, name) VALUES (?, ?, ?)",
        (chat_id, user_id, name or str(user_id)),
    )
    conn.commit()


def challenge_done(chat_id: int, user_id: int, name: str) -> bool:
    add_challenge_member(chat_id, user_id, name)
    row = conn.execute(
        "SELECT done_today FROM challenge_members WHERE chat_id = ? AND user_id = ?",
        (chat_id, user_id),
    ).fetchone()
    if row and row["done_today"]:
        return False
    conn.execute(
        "UPDATE challenge_members SET done_today = 1, streak = streak + 1 "
        "WHERE chat_id = ? AND user_id = ?",
        (chat_id, user_id),
    )
    conn.commit()
    return True


def challenge_top(chat_id: int):
    return conn.execute(
        "SELECT name, done_today, streak FROM challenge_members WHERE chat_id = ? "
        "ORDER BY done_today DESC, streak DESC, name LIMIT 15",
        (chat_id,),
    ).fetchall()


def reset_challenge_days() -> None:
    conn.execute("UPDATE challenge_members SET done_today = 0")
    conn.commit()


def expiring_pro_users(hours: int = 72):
    now = time.time()
    until = now + hours * 3600
    return conn.execute(
        "SELECT user_id, pro_until FROM users WHERE pro_until BETWEEN ? AND ?",
        (now, until),
    ).fetchall()
