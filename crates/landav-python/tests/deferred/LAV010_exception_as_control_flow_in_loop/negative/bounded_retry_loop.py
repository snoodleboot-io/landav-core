"""Fetch a URL with a bounded retry."""

import time


def fetch_with_retries(client, url):
    """Three attempts, and the handler is the retry mechanism itself."""
    for attempt in range(3):
        try:
            return client.fetch(url)
        except TimeoutError:
            time.sleep(2 ** attempt)
    raise TimeoutError(url)
