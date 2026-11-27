"""Минимальный async-клиент Lava Business API v2 (рекуррентные платежи).

Документация: https://developer.lava.ru/
Базовый URL: https://api.lava.ru/business

Подпись: HMAC-SHA256 от raw body байт в байт, заголовок Signature.
"""

import hashlib
import hmac
import json
import os
import time

import httpx

BASE_URL = "https://api.lava.ru/business"

SHOP_ID = os.getenv("LAVA_SHOP_ID", "")
SECRET_KEY = os.getenv("LAVA_SECRET_KEY", "")
PRODUCT_ID = os.getenv("LAVA_PRODUCT_ID", "")


def _headers(body: bytes) -> dict[str, str]:
    signature = hmac.new(SECRET_KEY.encode("utf-8"), body, hashlib.sha256).hexdigest()
    return {
        "Accept": "application/json",
        "Content-Type": "application/json",
        "Signature": signature,
    }


async def post(path: str, payload: dict) -> dict:
    """Подписывает именно ту строку, которую отправляет."""
    raw = json.dumps(payload, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    async with httpx.AsyncClient(timeout=20) as client:
        response = await client.post(
            BASE_URL + path,
            content=raw,
            headers=_headers(raw),
        )
    try:
        return response.json()
    except Exception:
        return {"error": f"HTTP {response.status_code}: {response.text[:300]}"}


async def create_subscription(user_id: int, email: str) -> tuple[str, str, dict]:
    """Создаёт consumer и подписку. Возвращает (order_id, pay_url, raw_response)."""
    if not SHOP_ID or not SECRET_KEY or not PRODUCT_ID:
        raise RuntimeError("Не заданы LAVA_SHOP_ID / LAVA_SECRET_KEY / LAVA_PRODUCT_ID")

    order_id = f"tg-{user_id}-{int(time.time())}"

    consumer = await post(
        "/recurrent/consumer/create",
        {
            "shopId": SHOP_ID,
            "consumerId": str(user_id),
            "email": email,
        },
    )
    lava_consumer_id = (consumer.get("data") or {}).get("consumerId") or str(user_id)

    subscription = await post(
        "/recurrent/subscription/subscribe",
        {
            "shopId": SHOP_ID,
            "productId": PRODUCT_ID,
            "consumerId": lava_consumer_id,
            "orderId": order_id,
        },
    )
    pay_url = (subscription.get("data") or {}).get("url", "")
    return order_id, pay_url, subscription


async def subscription_status(order_id: str) -> dict:
    """Проверяем оплату без webhook — раз в минуту из планировщика."""
    return await post(
        "/recurrent/subscription/status",
        {
            "shopId": SHOP_ID,
            "orderId": order_id,
        },
    )


def looks_paid(payload: dict) -> bool:
    """Эвристика по полям из OpenAPI Lava. Уточнить у поддержки точные статусы."""
    data = payload.get("data") or {}
    if payload.get("status_check") is True and payload.get("status") == 200:
        status = str(data.get("status", "")).lower()
        if status in {"activated", "success", "paid", "active"}:
            return True
    status = str(data.get("status", "")).lower()
    return status in {"activated", "success", "paid", "active"}
