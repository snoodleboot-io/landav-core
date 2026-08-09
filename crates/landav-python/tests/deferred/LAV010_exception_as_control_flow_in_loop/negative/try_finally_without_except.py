"""Write a buffer to every handle, flushing whatever happens."""


def write_all(handles, buffer):
    """``try``/``finally`` costs nothing until something actually raises."""
    for handle in handles:
        try:
            handle.write(buffer)
        finally:
            handle.flush()
