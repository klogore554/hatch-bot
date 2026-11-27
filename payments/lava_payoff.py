"""Вывод средств Lava, включая USDT в сети TRC-20.

Документация:
- https://developer.lava.ru/operations/payoff.api.create.md
- https://developer.lava.ru/operations/payoff.api.checkWallet.md
- https://developer.lava.ru/operations/payoff.api.tariffs.md
- https://developer.lava.ru/operations/payoff.api.info.md

Все запросы подписываются тем же секретным ключом, что и платёжные,
через lava.post() — подпись считается по raw body, который реально уходит.
"""

import os
import time
import uuid

from . import lava

PROFILE_ID = os.getenv("LAVA_PROFILE_ID", "")
WALLET_TO = os.getenv("LAVA_WALLET_TO", "")
SERVICE = "trc20_payoff"


def _profile() -> str:
    if not PROFILE_ID:
        raise RuntimeError("Не задан LAVA_PROFILE_ID — UUID профиля из ЛК Lava")
    return PROFILE_ID


async def check_wallet(wallet_to: str | None = None, service: str = SERVICE) -> dict:
    """Проверка кошелька перед выводом. Не списывает деньги."""
    return await lava.post(
        "/payoff/check-wallet",
        {"service": service, "walletTo": wallet_to or WALLET_TO},
    )


async def tariffs() -> dict:
    """Комиссии, лимиты и доступные направления вывода."""
    return await lava.post("/payoff/get-tariffs", {"profileId": _profile()})


async def create_payoff(
    amount: float,
    wallet_to: str | None = None,
    order_id: str | None = None,
    service: str = SERVICE,
    subtract: str | None = None,
    hook_url: str | None = None,
) -> dict:
    """Создаёт заявку на вывод. Деньги списываются — вызывать явно."""
    payload = {
        "profileId": _profile(),
        "amount": amount,
        "orderId": order_id or f"payoff-{int(time.time())}",
        "service": service,
        "walletTo": wallet_to or WALLET_TO,
    }
    if subtract in {"0", "1", "2"}:
        payload["subtract"] = subtract
    if hook_url:
        payload["hookUrl"] = hook_url
    return await lava.post("/payoff/create", payload)


async def payoff_info(order_id: str | None = None, payoff_id: str | None = None) -> dict:
    """Статус вывода: created / pending / success / rejected."""
    payload = {"profileId": _profile()}
    if order_id:
        payload["orderId"] = order_id
    if payoff_id:
        payload["payoffId"] = payoff_id
    if not order_id and not payoff_id:
        raise RuntimeError("Нужен orderId или payoffId")
    return await lava.post("/payoff/info", payload)


def new_order_id() -> str:
    return f"payoff-{uuid.uuid4().hex[:12]}"
