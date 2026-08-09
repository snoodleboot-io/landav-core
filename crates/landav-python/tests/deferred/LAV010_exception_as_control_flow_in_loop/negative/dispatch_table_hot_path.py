"""Dispatch messages through a handler table."""


def _build_handlers():
    return {}


_HANDLERS = _build_handlers()


def dispatch(messages):
    """EAFP, and here it is the *faster* spelling, not merely the idiomatic one.

    Setting up ``try`` costs nothing in CPython. ``_HANDLERS.get(kind)`` costs
    an attribute lookup and a Python-level call on every message, whereas
    ``_HANDLERS[kind]`` is a single opcode. Almost every message has a handler,
    so the unwind is taken rarely and ``get`` is a pessimisation of the hot
    path.
    """
    handled = 0
    for message in messages:
        try:
            handler = _HANDLERS[message.kind]
        except KeyError:
            continue
        handler(message)
        handled += 1
    return handled
