"""Drain a work queue, the O(1) way."""

from collections import deque


def drain(items):
    """``deque.popleft`` does not shift anything."""
    queue = deque(items)
    processed = []
    while queue:
        processed.append(queue.popleft())
    return processed
