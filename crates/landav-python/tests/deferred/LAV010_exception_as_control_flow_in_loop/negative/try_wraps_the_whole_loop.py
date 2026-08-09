"""Consume a stream, tolerating a reset."""

import logging

log = logging.getLogger(__name__)


def consume(stream, handle):
    """The setup is outside the loop and the raise happens at most once."""
    try:
        for chunk in stream:
            handle(chunk)
    except ConnectionResetError:
        log.warning("stream reset before end of body")
