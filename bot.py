"""Хаб-бот: привычки, стрики, групповой челлендж, рефералка и подписка через Lava.

Запуск: python bot.py
Переменные окружения задаются на хосте / в Railway, файл .env не используется.
"""

import asyncio
import logging
import os
import threading
import time
from datetime import datetime

import uvicorn
from aiogram import Bot, Dispatcher, F
from aiogram.exceptions import TelegramAPIError
from aiogram.filters import Command, CommandStart
from aiogram.fsm.context import FSMContext
from aiogram.fsm.state import State, StatesGroup
from aiogram.fsm.storage.memory import MemoryStorage
from aiogram.types import (
    BotCommand,
    InlineKeyboardButton,
    InlineKeyboardMarkup,
)
from aiogram.utils.keyboard import InlineKeyboardBuilder
from apscheduler.schedulers.asyncio import AsyncIOScheduler
from apscheduler.triggers.cron import CronTrigger
from fastapi import FastAPI

import db
import legal
from payments import lava

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s %(levelname)s %(name)s: %(message)s",
)
log = logging.getLogger("habit-bot")

PRESETS = {
    "run": "🏃 Утренний бег",
    "water": "💧 Пить воду",
    "read": "📚 Читать 10 минут",
    "med": "🧘 Медитация",
}


class SubState(StatesGroup):
    email = State()


class HabitState(StatesGroup):
    name = State()


app = FastAPI()


@app.get("/health")
async def health():
    return {"ok": True}


def main_keyboard() -> InlineKeyboardMarkup:
    kb = InlineKeyboardBuilder()
    kb.button(text="➕ Привычка", callback_data="menu_add")
    kb.button(text="✅ Отметить", callback_data="menu_done")
    kb.button(text="📊 Стрик", callback_data="menu_streak")
    kb.button(text="👥 Челлендж", callback_data="menu_group")
    kb.button(text="💎 Pro", callback_data="menu_pro")
    kb.button(text="📄 Документы", callback_data="menu_docs")
    kb.adjust(2, 2, 2)
    return kb.as_markup()


def docs_keyboard() -> InlineKeyboardMarkup:
    kb = InlineKeyboardBuilder()
    kb.button(text="Оферта", callback_data="doc_offer")
    kb.button(text="Возврат", callback_data="doc_refund")
    kb.button(text="Данные", callback_data="doc_privacy")
    kb.button(text="« Назад", callback_data="menu_back")
    kb.adjust(3, 1)
    return kb.as_markup()


def add_keyboard() -> InlineKeyboardMarkup:
    kb = InlineKeyboardBuilder()
    for key, label in PRESETS.items():
        kb.button(text=label, callback_data=f"add:{key}")
    kb.button(text="✍️ Своя", callback_data="add:custom")
    kb.button(text="« Назад", callback_data="menu_back")
    kb.adjust(2, 2, 1, 1)
    return kb.as_markup()


def habit_keyboard(user_id: int) -> InlineKeyboardMarkup:
    kb = InlineKeyboardBuilder()
    for habit in db.list_habits(user_id):
        kb.button(text=f"✅ {habit['name']}", callback_data=f"done:{habit['id']}")
    if db.is_pro(user_id):
        for habit in db.list_habits(user_id):
            kb.button(text=f"🗑 {habit['name']}", callback_data=f"del:{habit['id']}")
    kb.button(text="« Назад", callback_data="menu_back")
    kb.adjust(1)
    return kb.as_markup()


def streak_keyboard(user_id: int) -> InlineKeyboardMarkup:
    kb = InlineKeyboardBuilder()
    for habit in db.list_habits(user_id):
        value = db.streak(user_id, habit["id"])
        kb.button(text=f"{habit['name']} — {value} дн.", callback_data="noop")
    kb.button(text="« Назад", callback_data="menu_back")
    kb.adjust(1)
    return kb.as_markup()


def challenge_keyboard() -> InlineKeyboardMarkup:
    kb = InlineKeyboardBuilder()
    kb.button(text="✅ Я сделал", callback_data="gdone")
    kb.button(text="🏆 Топ", callback_data="gtop")
    kb.adjust(2)
    return kb.as_markup()


async def show_menu(target, user_id: int) -> None:
    pro = db.is_pro(user_id)
    mark = "💎 Pro" if pro else "🆓 Free"
    text = (
        f"<b>Хаб-бот</b> · {mark}\n\n"
        "Отмечай привычки, собирай стрики.\n"
        "Бесплатно — 1 привычка. Про — без лимита и со статистикой.\n\n"
        "Приведи 2 друзей — получи 3 дня Pro."
    )
    await target.answer(text, reply_markup=main_keyboard())


def register_handlers(dp: Dispatcher) -> None:
    @dp.message(CommandStart())
    async def cmd_start(message, state: FSMContext):
        parts = message.text.split(maxsplit=1)
        source = parts[1] if len(parts) > 1 else ""
        user_id = message.from_user.id
        db.add_user(user_id, message.from_user.username, source)

        if source.startswith("ref") and source[3:].isdigit():
            referrer = int(source[3:])
            if db.add_referral(referrer, user_id):
                try:
                    await message.bot.send_message(
                        referrer,
                        "🔥 Кто-то запустил бота по твоей ссылке. +3 дня Pro уже начислены.",
                    )
                except TelegramAPIError:
                    pass
            await message.answer("🎁 Ты пришёл по ссылке друга — +3 дня Pro!")

        await state.clear()
        await show_menu(message, user_id)

    @dp.message(Command("help"))
    async def cmd_help(message):
        await message.answer(
            "/start — меню\n"
            "/add — добавить привычку\n"
            "/today — отметить сегодня\n"
            "/streak — серии\n"
            "/remind 09:00 — время напоминания\n"
            "/referral — твоя ссылка\n"
            "/subscribe — подписка Pro\n"
            "/challenge — челлендж в группе (введи прямо в группе)\n"
            "/top — рейтинг группы"
        )

    @dp.message(Command("menu"))
    async def cmd_menu(message):
        await show_menu(message, message.from_user.id)

    @dp.callback_query(F.data == "menu_back")
    async def cb_back(callback):
        await show_menu(callback.message, callback.from_user.id)
        await callback.answer()

    @dp.callback_query(F.data == "noop")
    async def cb_noop(callback):
        await callback.answer()

    @dp.callback_query(F.data == "menu_docs")
    async def cb_docs(callback):
        await callback.message.edit_text("Документы:", reply_markup=docs_keyboard())
        await callback.answer()

    @dp.callback_query(F.data.startswith("doc_"))
    async def cb_doc(callback):
        text = legal.DOCUMENTS.get(callback.data)
        if not text:
            await callback.answer()
            return
        if callback.message.text != text:
            await callback.message.edit_text(text, reply_markup=docs_keyboard())
        await callback.answer()

    @dp.callback_query(F.data == "menu_add")
    async def cb_menu_add(callback):
        await callback.message.answer("Выбери привычку:", reply_markup=add_keyboard())
        await callback.answer()

    @dp.callback_query(F.data.startswith("add:"))
    async def cb_add(callback, state: FSMContext):
        key = callback.data.split(":", 1)[1]
        if key == "custom":
            await state.set_state(HabitState.name)
            await callback.message.answer("Напиши название привычки (до 40 символов):")
            await callback.answer()
            return
        name = PRESETS.get(key)
        if not name:
            await callback.answer("Не знаю такую привычку", show_alert=True)
            return
        ok = db.add_habit(callback.from_user.id, name)
        await callback.answer("Добавлено ✅" if ok else "Лимит", show_alert=not ok)
        if not ok:
            await callback.message.answer(
                "Бесплатно можно вести 1 привычку. Оформи /subscribe — и без лимита."
            )
        else:
            await callback.message.edit_text(f"Добавил: {name}")

    @dp.message(HabitState.name)
    async def habit_name(message, state: FSMContext):
        name = (message.text or "").strip()[:40]
        if not name:
            await message.answer("Пусто. Напиши ещё раз или /cancel")
            return
        ok = db.add_habit(message.from_user.id, name)
        await message.answer(
            f"Добавил: {name}" if ok else "Бесплатно только 1 привычка. /subscribe"
        )
        await state.clear()

    @dp.message(Command("cancel"))
    async def cmd_cancel(message, state: FSMContext):
        await state.clear()
        await message.answer("Отменил.")

    @dp.message(Command("add"))
    async def cmd_add(message):
        await message.answer("Выбери привычку:", reply_markup=add_keyboard())

    @dp.message(Command("today"))
    @dp.callback_query(F.data == "menu_done")
    async def show_today(event):
        user_id = event.from_user.id
        habits = db.list_habits(user_id)
        target = event.message if hasattr(event, "message") else event
        if not habits:
            await target.answer("Сначала добавь привычку: /add")
            return
        await target.answer("Что сделал сегодня?", reply_markup=habit_keyboard(user_id))
        if hasattr(event, "answer"):
            await event.answer()

    @dp.callback_query(F.data.startswith("done:"))
    async def cb_done(callback):
        habit_id = int(callback.data.split(":", 1)[1])
        fresh = db.mark_done(callback.from_user.id, habit_id)
        if fresh:
            value = db.streak(callback.from_user.id, habit_id)
            await callback.message.edit_text(f"🔥 Отмечено! Стрик: {value} дн.")
            await callback.answer("Красавчик")
        else:
            await callback.answer("Уже отмечено сегодня", show_alert=True)

    @dp.callback_query(F.data.startswith("del:"))
    async def cb_del(callback):
        habit_id = int(callback.data.split(":", 1)[1])
        db.delete_habit(callback.from_user.id, habit_id)
        await callback.message.edit_text("Удалил.")
        await callback.answer()

    @dp.message(Command("streak"))
    @dp.callback_query(F.data == "menu_streak")
    async def cmd_streak(event):
        user_id = event.from_user.id
        target = event.message if hasattr(event, "message") else event
        habits = db.list_habits(user_id)
        if not habits:
            await target.answer("Привычек пока нет — /add")
        else:
            lines = [
                f"{h['name']}: <b>{db.streak(user_id, h['id'])}</b> дн."
                for h in habits
            ]
            await target.answer("Текущие серии:\n" + "\n".join(lines))
        if hasattr(event, "answer"):
            await event.answer()

    @dp.message(Command("remind"))
    async def cmd_remind(message):
        parts = message.text.split(maxsplit=1)
        if len(parts) != 2:
            await message.answer("Пример: /remind 09:00")
            return
        value = parts[1].strip()
        try:
            datetime.strptime(value, "%H:%M")
        except ValueError:
            await message.answer("Нужен формат ЧЧ:ММ, например 09:00")
            return
        db.set_reminder(message.from_user.id, value)
        await message.answer(f"Напоминание каждый день в {value} (МСК).")

    @dp.message(Command("referral"))
    async def cmd_referral(message):
        me = await message.bot.get_me()
        link = f"https://t.me/{me.username}?start=ref{message.from_user.id}"
        await message.answer(
            "Твоя ссылка:\n"
            f"<code>{link}</code>\n\n"
            "За каждого друга, который запустит бота, ты и он получаете +3 дня Pro."
        )

    @dp.message(Command("pro"))
    @dp.callback_query(F.data == "menu_pro")
    async def cmd_pro(event):
        target = event.message if hasattr(event, "message") else event
        until = db.pro_until(event.from_user.id)
        status = (
            datetime.fromtimestamp(until, db.MSK).strftime("%d.%m.%Y")
            if until > time.time()
            else "не активен"
        )
        await target.answer(
            "<b>Хаб-бот Pro</b> — 299 ₽/мес\n\n"
            "• безлимитные привычки\n"
            "• полная статистика и серии\n"
            "• гибкое время напоминаний\n"
            "• +3 дня за каждого друга\n\n"
            f"Твой Pro: {status}",
            reply_markup=InlineKeyboardMarkup(
                inline_keyboard=[
                    [InlineKeyboardButton(text="💳 Оформить", callback_data="go_sub")]
                ]
            ),
        )
        if hasattr(event, "answer"):
            await event.answer()

    @dp.callback_query(F.data == "go_sub")
    async def cb_go_sub(callback, state: FSMContext):
        await state.set_state(SubState.email)
        await callback.message.answer(
            "Напиши email, который указан в «Моём налоге» — туда пойдёт чек:"
        )
        await callback.answer()

    @dp.message(Command("subscribe"))
    async def cmd_subscribe(message, state: FSMContext):
        await state.set_state(SubState.email)
        await message.answer(
            "Напиши email, который указан в «Моём налоге» — туда пойдёт чек:"
        )

    @dp.message(SubState.email)
    async def sub_email(message, state: FSMContext):
        email = (message.text or "").strip()
        if "@" not in email:
            await message.answer("Похоже, это не email. Попробуй ещё раз или /cancel")
            return
        try:
            order_id, pay_url, raw = await lava.create_subscription(
                message.from_user.id, email
            )
        except Exception as exc:
            log.exception("Не удалось создать подписку")
            await message.answer(f"Ошибка оплаты: {exc}. Напиши в поддержку бота.")
            await state.clear()
            return
        if not pay_url:
            await message.answer(
                "Lava не вернула ссылку на оплату. Проверь настройки продукта."
            )
            log.warning("Ответ Lava без url: %s", raw)
        else:
            db.add_order(order_id, message.from_user.id)
            await message.answer(
                "Ссылка на оплату:\n"
                f"{pay_url}\n\n"
                "После оплаты доступ продлится сам — проверяю раз в минуту."
            )
        await state.clear()

    @dp.callback_query(F.data == "menu_group")
    async def cb_group(callback):
        await callback.message.answer(
            "Добавь бота в группу и напиши там /challenge.\n"
            "В группе будет рейтинг, а участники сами будут звать друзей.",
        )
        await callback.answer()

    @dp.message(Command("challenge"))
    async def cmd_challenge(message):
        if message.chat.type == "private":
            await message.answer("Команду /challenge надо написать прямо в группе.")
            return
        ends = db.create_challenge(message.chat.id, days=7)
        db.add_challenge_member(
            message.chat.id, message.from_user.id, message.from_user.first_name
        )
        until = datetime.fromtimestamp(ends, db.MSK).strftime("%d.%m.%Y")
        await message.answer(
            f"🏁 Челлендж на 7 дней запущен!\nДо {until}.\n\n"
            "Каждый день жми «Я сделал». В конце недели покажу победителей.\n"
            "Зови друзей — им тоже захочется быть в рейтинге.",
            reply_markup=challenge_keyboard(),
        )

    @dp.callback_query(F.data == "gdone")
    async def cb_gdone(callback):
        chat_id = callback.message.chat.id
        if not db.challenge_active(chat_id):
            await callback.answer("Челлендж закончился. Напиши /challenge", show_alert=True)
            return
        name = callback.from_user.first_name or str(callback.from_user.id)
        fresh = db.challenge_done(chat_id, callback.from_user.id, name)
        await callback.answer("Отметил ✅" if fresh else "Сегодня уже отмечен")

    @dp.message(Command("top"))
    @dp.callback_query(F.data == "gtop")
    async def cmd_top(event):
        target = event.message if hasattr(event, "message") else event
        chat_id = target.chat.id
        rows = db.challenge_top(chat_id)
        if not rows:
            await target.answer("В этом чате ещё нет челленджа.")
        else:
            lines = []
            for index, row in enumerate(rows, start=1):
                mark = "✅" if row["done_today"] else "⬜"
                lines.append(
                    f"{index}. {mark} {row['name']} — {row['streak']} дн."
                )
            await target.answer("🏆 Рейтинг:\n" + "\n".join(lines))
        if hasattr(event, "answer"):
            await event.answer()


async def check_payments(bot: Bot) -> None:
    try:
        for order in db.unpaid_orders():
            result = await lava.subscription_status(order["order_id"])
            if lava.looks_paid(result):
                user_id = db.pay_order(order["order_id"])
                if user_id:
                    await bot.send_message(
                        user_id,
                        "💎 Оплата прошла! Pro продлён на 30 дней. Спасибо!",
                    )
    except Exception:
        log.exception("Ошибка проверки платежей")


async def send_reminders(bot: Bot) -> None:
    try:
        hhmm = db.now_msk().strftime("%H:%M")
        day = db.today()
        for row in db.users_to_remind(hhmm, day):
            db.mark_reminded(row["user_id"], day)
            try:
                await bot.send_message(
                    row["user_id"],
                    "Сегодня тренировка? Не забудь отметить привычку ✅",
                    reply_markup=InlineKeyboardMarkup(
                        inline_keyboard=[
                            [InlineKeyboardButton(text="✅ Открыть", callback_data="menu_done")]
                        ]
                    ),
                )
            except TelegramAPIError:
                pass
    except Exception:
        log.exception("Ошибка напоминаний")


async def warn_expiring(bot: Bot) -> None:
    try:
        marker = "warn-" + db.today()
        for row in db.expiring_pro_users():
            user_id = row["user_id"]
            if db.was_reminded(user_id, marker):
                continue
            db.mark_reminded(user_id, marker)
            try:
                await bot.send_message(
                    user_id,
                    "Pro заканчивается через 3 дня. Продлить можно в /subscribe.",
                )
            except TelegramAPIError:
                pass
    except Exception:
        log.exception("Ошибка предупреждения об окончании Pro")


def run_health_server() -> None:
    uvicorn.run(
        app,
        host="0.0.0.0",
        port=int(os.getenv("PORT", "8000")),
        log_level="warning",
        access_log=False,
    )


async def main() -> None:
    token = os.getenv("BOT_TOKEN", "")
    if not token:
        raise RuntimeError("Не задан BOT_TOKEN — задай его на хосте/Railway")

    db.init_db()
    bot = Bot(token=token, parse_mode="HTML")
    dp = Dispatcher(storage=MemoryStorage())
    register_handlers(dp)

    await bot.set_my_commands(
        [
            BotCommand(command="start", description="Меню"),
            BotCommand(command="add", description="Добавить привычку"),
            BotCommand(command="today", description="Отметить сегодня"),
            BotCommand(command="streak", description="Серии"),
            BotCommand(command="referral", description="Ссылка для друзей"),
            BotCommand(command="subscribe", description="Pro"),
            BotCommand(command="challenge", description="Челлендж в группе"),
            BotCommand(command="help", description="Помощь"),
        ]
    )

    scheduler = AsyncIOScheduler()
    scheduler.add_job(check_payments, "interval", seconds=60, args=[bot])
    scheduler.add_job(send_reminders, "interval", seconds=60, args=[bot])
    scheduler.add_job(warn_expiring, "interval", hours=6, args=[bot])
    scheduler.add_job(
        db.reset_challenge_days,
        CronTrigger(hour=0, minute=5, timezone=db.MSK),
    )
    scheduler.start()

    if os.getenv("HEALTH_SERVER", "1") == "1":
        threading.Thread(target=run_health_server, daemon=True).start()

    log.info("Бот запущен")
    try:
        await dp.start_polling(bot, allowed_updates=dp.resolve_used_update_types())
    finally:
        scheduler.shutdown(wait=False)
        await bot.session.close()


if __name__ == "__main__":
    asyncio.run(main())
