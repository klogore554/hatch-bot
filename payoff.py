"""Ручной вывод средств из Lava на TRC-20 кошелёк.

Не запускается ботом. Только по явному вызову:

    python payoff.py check
    python payoff.py tariffs
    python payoff.py create 1000
    python payoff.py info payoff-xxxxxxxxxxxx

Переменные задаются на хосте, файл .env не используется:
    LAVA_SECRET_KEY, LAVA_PROFILE_ID, LAVA_WALLET_TO
"""

import asyncio
import json
import os
import sys

from payments import lava_payoff


def show(result: dict) -> None:
    print(json.dumps(result, ensure_ascii=False, indent=2))


async def main() -> None:
    if not os.getenv("LAVA_SECRET_KEY"):
        raise RuntimeError("Не задан LAVA_SECRET_KEY")

    args = sys.argv[1:]
    if not args:
        print(__doc__)
        return

    command = args[0].lower()

    if command == "check":
        show(await lava_payoff.check_wallet())

    elif command == "tariffs":
        show(await lava_payoff.tariffs())

    elif command == "create":
        if len(args) < 2:
            raise SystemExit("Пример: python payoff.py create 1000")
        amount = float(args[1])
        if amount <= 0:
            raise SystemExit("Сумма должна быть больше нуля")
        wallet = os.getenv("LAVA_WALLET_TO", "")
        if not wallet.startswith("T") or len(wallet) != 34:
            raise SystemExit(
                "LAVA_WALLET_TO должен быть TRON-адресом: начинается с T, 34 символа"
            )
        order_id = lava_payoff.new_order_id()
        if "--yes" not in args:
            print(f"Сумма: {amount}, кошелёк: {wallet}, orderId: {order_id}")
            if input("Напиши yes для подтверждения вывода: ").strip().lower() != "yes":
                raise SystemExit("Отменено")
        show(await lava_payoff.create_payoff(amount, order_id=order_id))

    elif command == "info":
        if len(args) < 2:
            raise SystemExit("Пример: python payoff.py info payoff-xxxxxxxxxxxx")
        show(await lava_payoff.payoff_info(order_id=args[1]))

    else:
        print(__doc__)


if __name__ == "__main__":
    asyncio.run(main())
