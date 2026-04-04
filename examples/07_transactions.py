"""
Ryx ORM — Example 07: Transactions

This example covers:
  - Basic transactions (commit on success, rollback on error)
  - Nested transactions via SAVEPOINTs
  - Explicit savepoint creation and rollback
  - Transaction isolation — concurrent transactions don't see uncommitted data
  - Mixing transactions with bulk operations
  - get_active_transaction() — check if inside a transaction

Run with:
    uv run python examples/07_transactions.py
"""

import asyncio
import os
from pathlib import Path

import ryx
from ryx import Model, CharField, IntField, transaction, get_active_transaction
from ryx.migrations import MigrationRunner
from ryx.exceptions import RyxError


DB_PATH = Path(__file__).parent.parent / "ryx_examples.sqlite3"
DATABASE_URL = f"sqlite://{DB_PATH}?mode=rwc"
os.environ["RYX_DATABASE_URL"] = DATABASE_URL


#
#  MODELS
#
class Account(Model):
    class Meta:
        table_name = "ex7_accounts"

    name = CharField(max_length=100)
    balance = IntField(default=0)


class TransferLog(Model):
    class Meta:
        table_name = "ex7_transfers"

    from_account = CharField(max_length=100)
    to_account = CharField(max_length=100)
    amount = IntField()


async def setup() -> None:
    await ryx.setup(DATABASE_URL)
    runner = MigrationRunner([Account, TransferLog])
    await runner.migrate()

    # Clean
    await TransferLog.objects.bulk_delete()
    await Account.objects.bulk_delete()


#
#  BASIC TRANSACTIONS
#
async def demo_basic_commit() -> None:
    print("\n" + "=" * 60)
    print("Basic Transaction — Commit on Success")
    print("=" * 60)

    async with transaction():
        alice = await Account.objects.create(name="Alice", balance=1000)
        bob = await Account.objects.create(name="Bob", balance=500)
        print(f"Created: {alice.name} (${alice.balance}), {bob.name} (${bob.balance})")

    # Outside the transaction — data is committed
    count = await Account.objects.count()
    print(f"After commit: {count} accounts in DB")


async def demo_basic_rollback() -> None:
    print("\n" + "=" * 60)
    print("Basic Transaction — Rollback on Exception")
    print("=" * 60)

    count_before = await Account.objects.count()

    try:
        async with transaction():
            await Account.objects.create(name="Temp", balance=0)
            print("Created temp account (will be rolled back)")
            raise ValueError("Something went wrong!")
    except ValueError:
        pass  # Expected

    count_after = await Account.objects.count()
    print(
        f"After rollback: {count_after} accounts (was {count_before}) — temp account gone"
    )


#
#  NESTED TRANSACTIONS (SAVEPOINTs)
#
async def demo_nested_commit() -> None:
    print("\n" + "=" * 60)
    print("Nested Transactions — Both Commit")
    print("=" * 60)

    async with transaction():
        await Account.objects.create(name="Outer", balance=100)

        async with transaction():
            # Inner transaction creates a SAVEPOINT
            await Account.objects.create(name="Inner", balance=200)
            print("Inner transaction committed (savepoint released)")

        # Inner changes are visible
        count = await Account.objects.count()
        print(f"After inner commit: {count} accounts")

    # Both are committed
    count = await Account.objects.count()
    print(f"After outer commit: {count} accounts total")


async def demo_nested_rollback() -> None:
    print("\n" + "=" * 60)
    print("Nested Transactions — Inner Rolls Back, Outer Commits")
    print("=" * 60)

    async with transaction():
        await Account.objects.create(name="Outer2", balance=100)
        count_before = await Account.objects.count()
        print(f"Before inner: {count_before} accounts")

        try:
            async with transaction():
                await Account.objects.create(name="Inner2", balance=200)
                raise ValueError("Inner transaction failed!")
        except ValueError:
            pass  # Inner rolled back

        # Inner changes are gone, outer still active
        count_after = await Account.objects.count()
        print(f"After inner rollback: {count_after} accounts (Inner2 gone)")

    # Only outer changes survive
    count = await Account.objects.count()
    print(f"After outer commit: {count} accounts (only Outer2)")


#
#  EXPLICIT SAVEPOINTS
#
async def demo_explicit_savepoint() -> None:
    print("\n" + "=" * 60)
    print("Explicit Savepoints — Partial Rollback")
    print("=" * 60)

    async with transaction() as tx:
        alice = await Account.objects.create(name="Alice2", balance=1000)
        print(f"Created Alice: ${alice.balance}")

        # Create a savepoint before risky operations
        await tx.savepoint("before_transfers")

        # Simulate a transfer
        await Account.objects.filter(name="Alice2").update(balance=800)
        await TransferLog.objects.create(
            from_account="Alice2", to_account="Bob2", amount=200
        )
        print("Transfer recorded")

        # Oops — something went wrong, rollback to savepoint
        await tx.rollback_to("before_transfers")
        print("Rolled back to savepoint — transfer undone")

    # Check final state
    alice = await Account.objects.get(name="Alice2")
    print(f"Alice's balance after rollback: ${alice.balance} (still 1000)")

    logs = await TransferLog.objects.count()
    print(f"Transfer logs: {logs} (rollback removed the log too)")


#
#  TRANSACTION ISOLATION
#
async def demo_isolation() -> None:
    print("\n" + "=" * 60)
    print("Transaction Isolation")
    print("=" * 60)

    # Create a base account
    await Account.objects.create(name="Shared", balance=500)

    async def tx_a():
        async with transaction():
            # This update is NOT visible outside this transaction
            await Account.objects.filter(name="Shared").update(balance=999)
            inside = await Account.objects.get(name="Shared")
            print(f"  TX A sees balance: ${inside.balance}")
            # Don't commit yet — let tx_b read
            return inside.balance

    async def tx_b():
        # This transaction runs independently
        result = await Account.objects.get(name="Shared")
        print(f"  TX B sees balance: ${result.balance}")
        return result.balance

    # Run tx_a but don't let it commit before tx_b reads
    # Note: SQLite's default isolation means tx_b sees committed data only
    await tx_a()
    # tx_a committed, now tx_b sees the update
    await tx_b()


#
#  TRANSACTIONS WITH BULK OPERATIONS
#
async def demo_bulk_in_transaction() -> None:
    print("\n" + "=" * 60)
    print("Bulk Operations Inside Transactions")
    print("=" * 60)

    async with transaction():
        # Bulk create
        accounts = [Account(name=f"Bulk User {i}", balance=i * 100) for i in range(5)]
        await Account.objects.bulk_create(accounts)
        print(f"Bulk created: {len(accounts)} accounts")

        # Bulk update
        all_bulk = await Account.objects.filter(name__startswith="Bulk User")
        for acc in all_bulk:
            acc.balance += 50
        await Account.objects.bulk_update(all_bulk, ["balance"])
        print(f"Bulk updated: {len(all_bulk)} accounts")

        # Bulk delete — remove even-numbered users
        even_users = await Account.objects.filter(name__endswith="0")
        if even_users:
            deleted = await Account.objects.bulk_delete(even_users)
            print(f"Bulk deleted: {deleted} accounts")

    # Verify
    remaining = await Account.objects.filter(name__startswith="Bulk User")
    print(f"Remaining bulk accounts: {len(remaining)}")
    for acc in remaining:
        print(f"  {acc.name}: ${acc.balance}")


#
#  GET_ACTIVE_TRANSACTION
#
async def demo_get_active() -> None:
    print("\n" + "=" * 60)
    print("get_active_transaction()")
    print("=" * 60)

    # Outside transaction
    tx = get_active_transaction()
    print(f"Outside transaction: {tx}")

    # Inside transaction
    async with transaction():
        tx = get_active_transaction()
        print(f"Inside transaction:  {tx is not None}")


#
#  REAL-WORLD: BANK TRANSFER
#
async def bank_transfer(from_name: str, to_name: str, amount: int) -> bool:
    """Atomically transfer money between accounts.

    Returns True on success, False on failure.
    """
    try:
        async with transaction():
            sender = await Account.objects.get(name=from_name)
            receiver = await Account.objects.get(name=to_name)

            if sender.balance < amount:
                raise RyxError("Insufficient funds")

            sender.balance -= amount
            receiver.balance += amount
            await sender.save()
            await receiver.save()

            await TransferLog.objects.create(
                from_account=from_name,
                to_account=to_name,
                amount=amount,
            )
            return True
    except Exception as e:
        print(f"  Transfer failed: {e}")
        return False


async def demo_bank_transfer() -> None:
    print("\n" + "=" * 60)
    print("Real-World Example: Atomic Bank Transfer")
    print("=" * 60)

    # Setup
    await Account.objects.bulk_delete()
    await TransferLog.objects.bulk_delete()
    await Account.objects.create(name="Alice", balance=1000)
    await Account.objects.create(name="Bob", balance=500)

    # Successful transfer
    print("\nTransfer $200 from Alice to Bob:")
    success = await bank_transfer("Alice", "Bob", 200)
    print(f"  Success: {success}")

    alice = await Account.objects.get(name="Alice")
    bob = await Account.objects.get(name="Bob")
    print(f"  Alice: ${alice.balance}, Bob: ${bob.balance}")

    # Failed transfer — insufficient funds
    print("\nTransfer $2000 from Bob to Alice:")
    success = await bank_transfer("Bob", "Alice", 2000)
    print(f"  Success: {success}")

    # Balances should be unchanged
    alice = await Account.objects.get(name="Alice")
    bob = await Account.objects.get(name="Bob")
    print(f"  Alice: ${alice.balance}, Bob: ${bob.balance} (unchanged)")


#
#  MAIN
#
async def main() -> None:
    print("Ryx ORM — Example 07: Transactions")
    await setup()

    await demo_basic_commit()
    await demo_basic_rollback()
    await demo_nested_commit()
    await demo_nested_rollback()
    await demo_explicit_savepoint()
    await demo_isolation()
    await demo_bulk_in_transaction()
    await demo_get_active()
    await demo_bank_transfer()

    print("\nDone!")


if __name__ == "__main__":
    asyncio.run(main())
