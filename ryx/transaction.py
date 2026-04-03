"""
Ryx ORM — Transaction Context Manager

Usage (basic):
  async with Ryx.transaction():
      await Post.objects.filter(pk=1).update(active=False)
      await Comment.objects.filter(post_id=1).delete()
      # auto-commit on clean exit, auto-rollback on exception

Usage (with explicit handle):
  async with Ryx.transaction() as tx:
      await Post.objects.filter(pk=1).update(views=100)
      await tx.savepoint("sp1")
      try:
          await Comment.objects.filter(spam=True).delete()
      except Exception:
          await tx.rollback_to("sp1")

Usage (nested via savepoints):
  async with Ryx.transaction() as outer:
      ...
      async with Ryx.transaction() as inner:
          # inner auto-creates a SAVEPOINT and releases/rolls back on exit
          ...

Design notes:
  - The Rust side owns the actual sqlx::Transaction.
  - Python context manager wraps it with commit-on-exit / rollback-on-error.
  - Nesting works: if there is already an active transaction on the current
    task, inner `transaction()` calls create a SAVEPOINT instead of BEGIN.
  - We use contextvars.ContextVar to propagate the active transaction through
    the async call stack without passing it explicitly to every ORM call.
    Future: ORM operations auto-enlist in the active transaction.
"""

from __future__ import annotations

# import asyncio
import contextvars
import logging
from typing import Optional

from ryx import ryx_core as _core

logger = logging.getLogger("Ryx.transaction")

# ContextVar: holds the currently active transaction handle (if any)
# for the current async task. This enables auto-enlistment in a future version.
_active_tx: contextvars.ContextVar[Optional[object]] = contextvars.ContextVar(
    "Ryx_active_tx", default=None
)

# ContextVar: holds the current TransactionContext object for Python-level
# rollback/undo bookkeeping (in case DB-level enlistment is not supported yet).
_active_tx_context: contextvars.ContextVar[Optional["TransactionContext"]] = (
    contextvars.ContextVar("Ryx_active_tx_context", default=None)
)


###
##      TRANSACTION CONTEXT
####
class TransactionContext:
    """Async context manager for database transactions.

    Created by :func:`transaction`. Do not instantiate directly.

    On ``__aexit__``:
      - No exception → ``COMMIT``
      - Exception raised → ``ROLLBACK``

    The ``TransactionHandle`` (from Rust) is exposed as the context manager
    value so callers can use explicit ``savepoint()`` / ``rollback_to()``.
    """

    def __init__(self) -> None:
        self._handle = None  # set in __aenter__
        self._savepoint_name: Optional[str] = None
        self._outer_token = None  # for ContextVar reset
        self._previous_tx = None  # restore on __aexit__
        self._ops: list[tuple[str, str, Optional[int]]] = []
        self._parent_context: Optional["TransactionContext"] = None

    async def __aenter__(self):
        outer = _active_tx.get()

        if outer is not None:
            #  Nested transaction → SAVEPOINT
            # We reuse the outer transaction's connection and create a named
            # savepoint. The name is unique per nesting level.
            sp_name = f"_Ryx_sp_{id(self)}"
            self._savepoint_name = sp_name
            await outer.savepoint(sp_name)
            self._handle = outer
            logger.debug("Nested transaction: created savepoint %s", sp_name)
        else:
            # Outermost transaction → BEGIN
            self._handle = await _core.begin_transaction()
            logger.debug("Transaction BEGIN")

        self._outer_token = _active_tx.set(self._handle)
        self._previous_tx = outer
        _core._set_active_transaction(self._handle)
        return self._handle

    async def __aexit__(self, exc_type, exc_val, exc_tb):
        _active_tx.reset(self._outer_token)
        _core._set_active_transaction(self._previous_tx)

        if self._savepoint_name:
            # Nested: release or rollback SAVEPOINT
            if exc_type is None:
                logger.debug("Nested TX: releasing savepoint %s", self._savepoint_name)
                try:
                    await self._handle.release_savepoint(self._savepoint_name)
                except Exception:
                    pass
            else:
                logger.debug(
                    "Nested TX: rolling back to savepoint %s", self._savepoint_name
                )
                try:
                    await self._handle.rollback_to(self._savepoint_name)
                except Exception:
                    pass
        else:
            # Outermost: COMMIT or ROLLBACK
            if exc_type is None:
                logger.debug("Transaction COMMIT")
                await self._handle.commit()
            else:
                logger.debug("Transaction ROLLBACK (due to %s)", exc_type.__name__)
                await self._handle.rollback()

        # Do not suppress the exception — let it propagate.
        return False


def transaction() -> TransactionContext:
    """Return an async context manager for database transactions.

    Usage::

        async with Ryx.transaction():
            await Post.objects.create(title="Atomic post")
            await Tag.objects.create(name="python")

        # With explicit handle for savepoints:
        async with Ryx.transaction() as tx:
            await Order.objects.create(total=99.99)
            await tx.savepoint("before_items")
            try:
                for item in items:
                    await OrderItem.objects.create(**item)
            except ValidationError:
                await tx.rollback_to("before_items")
                raise

    Nesting::

        async with Ryx.transaction():           # BEGIN
            ...
            async with Ryx.transaction():       # SAVEPOINT _Ryx_sp_...
                ...                               # RELEASE or ROLLBACK TO sp
            ...                                   # COMMIT / ROLLBACK

    Returns:
        :class:`TransactionContext` — an async context manager.
    """
    return TransactionContext()


def get_active_transaction():
    """Return the currently active transaction handle, or None.

    Useful when you want to conditionally enlist in an existing transaction
    without creating a new one.

    Example::

        tx = Ryx.get_active_transaction()
        if tx:
            # we're inside a transaction — the next ORM call auto-enlists
            pass
    """
    return _active_tx.get()
