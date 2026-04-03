"""
Rxy ORM — Signal / Hook System

Two complementary mechanisms:

1. HOOKS (per-model, instance-level, override in subclass):
     async def before_save(self, created: bool) -> None: ...
     async def after_save(self,  created: bool) -> None: ...
     async def before_delete(self)              -> None: ...
     async def after_delete(self)               -> None: ...
     async def clean(self)                      -> None: ...  (validation)

   These are the most common pattern — override in your Model subclass.

2. SIGNALS (global, observer pattern, connect many receivers):
     pre_save.connect(receiver_fn, sender=MyModel)
     post_save.connect(receiver_fn)     # no sender = fires for ALL models
     pre_delete.connect(receiver_fn, sender=MyModel)
     post_delete.connect(receiver_fn, sender=MyModel)

   Signals are process-global and designed for decoupled side-effects
   (cache invalidation, audit logs, webhooks, etc.).

Receiver functions must be async coroutines::

  async def on_post_save(sender, instance, created, **kwargs):
      if created:
          await send_welcome_email(instance)

  post_save.connect(on_post_save, sender=User)

Signal firing order:
  1. before_save hook  (instance method)
  2. pre_save  signal  (global observers)
  3. SQL executed
  4. after_save hook   (instance method)
  5. post_save signal  (global observers)
"""

from __future__ import annotations

# import asyncio
import inspect
import logging
import weakref
from typing import Any, Callable, Optional, Type

logger = logging.getLogger("Rxy.signals")


####
###      BASE SIGNAL CLASS
#####
class Signal:
    """A process-global, async, multi-receiver signal.

    Receivers are async callables. They are stored as weak references by
    default so that connecting a method to a signal doesn't prevent garbage
    collection of the object.

    Usage::

        # Connect
        post_save.connect(my_receiver, sender=Post)

        # Disconnect
        post_save.disconnect(my_receiver, sender=Post)

        # Fire (called by the ORM internals — users rarely fire signals)
        await post_save.send(sender=Post, instance=post, created=True)

    Args:
        name: Human-readable signal name (for logging).
    """

    def __init__(self, name: str) -> None:
        self.name = name
        # List of (sender_class_or_None, weak_ref_to_receiver)
        self._receivers: list[tuple[Optional[type], Any]] = []

    def connect(
        self,
        receiver: Callable,
        *,
        sender: Optional[Type] = None,
        weak: bool = True,
    ) -> None:
        """Register a receiver for this signal.

        Args:
            receiver: An async callable. Must accept ``(sender, **kwargs)``.
            sender:   If given, only fire for this specific Model class.
                      If None, fire for ALL senders.
            weak:     If True (default), store as a weak reference so the
                      receiver is automatically removed when it is garbage-
                      collected. Set to False for module-level functions that
                      will never be GC'd.
        """
        if not inspect.iscoroutinefunction(receiver):
            raise TypeError(
                f"Signal receiver must be an async function. "
                f"Got: {receiver!r}"
            )
        if weak:
            try:
                # Bound methods need weakref.WeakMethod
                ref = weakref.WeakMethod(receiver)   # type: ignore[arg-type]
            except TypeError:
                ref = weakref.ref(receiver)          # type: ignore[assignment]
        else:
            # Wrap in a lambda that always returns the receiver so the code
            # below works uniformly regardless of weak/strong.
            ref = lambda: receiver  # noqa: E731

        self._receivers.append((sender, ref))
        logger.debug("Signal %s: connected %r (sender=%r)", self.name, receiver, sender)

    def disconnect(
        self,
        receiver: Callable,
        *,
        sender: Optional[Type] = None,
    ) -> bool:
        """Remove a receiver from this signal.

        Returns True if the receiver was found and removed, False otherwise.
        """
        initial_len = len(self._receivers)
        self._receivers = [
            (s, ref) for (s, ref) in self._receivers
            if not (s is sender and self._is_same_receiver(ref, receiver))
        ]
        removed = len(self._receivers) < initial_len
        if removed:
            logger.debug("Signal %s: disconnected %r", self.name, receiver)
        return removed

    async def send(self, sender: type, **kwargs: Any) -> list[Any]:
        """Fire the signal and await all matching receivers.

        Receivers are called concurrently (asyncio.gather). Exceptions in one
        receiver do NOT prevent others from running — they are logged and
        collected as results.

        Args:
            sender:  The Model class that is sending the signal.
            **kwargs: Passed through to every receiver.

        Returns:
            List of (receiver, result_or_exception) pairs.
        """
        # Collect live receivers that match this sender
        live: list[Callable] = []
        dead: list[int] = []

        for i, (s, ref) in enumerate(self._receivers):
            fn = ref()
            if fn is None:
                dead.append(i)
                continue
            if s is None or s is sender:
                live.append(fn)

        # Remove dead weak references
        for i in reversed(dead):
            self._receivers.pop(i)

        if not live:
            return []

        results = []
        for fn in live:
            try:
                result = await fn(sender=sender, **kwargs)
                results.append((fn, result))
            except Exception as exc:
                logger.exception(
                    "Signal %s: receiver %r raised %r",
                    self.name, fn, exc,
                )
                results.append((fn, exc))

        return results

    def _is_same_receiver(self, ref: Any, fn: Callable) -> bool:
        """Compare a stored reference to a callable."""

        stored = ref()
        if stored is None:
            return False
        return stored == fn

    def __repr__(self) -> str:
        return f"<Signal: {self.name}>"


####    BUILTIN SIGNALS
pre_save = Signal("pre_save")
post_save = Signal("post_save")
pre_delete = Signal("pre_delete")
post_delete = Signal("post_delete")

####    Fired before/after a bulk QuerySet.update() call
pre_update  = Signal("pre_update")
post_update = Signal("post_update")

####    Fired before/after a bulk QuerySet.delete() call
pre_bulk_delete  = Signal("pre_bulk_delete")
post_bulk_delete = Signal("post_bulk_delete")


# Decorator shortcut
def receiver(signal: Signal, *, sender: Optional[Type] = None, weak: bool = True):
    """Decorator shortcut for connecting a receiver to a signal.

    Usage::

        @receiver(post_save, sender=Post)
        async def notify_on_new_post(sender, instance, created, **kwargs):
            if created:
                await push_notification(instance)
    """
    def decorator(fn: Callable) -> Callable:
        signal.connect(fn, sender=sender, weak=weak)
        return fn
    return decorator
