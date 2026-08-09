"""Render a three-character permission string."""


def format_flags(flags):
    """Three iterations, fixed at compile time."""
    text = ""
    for name in ("read", "write", "exec"):
        text += name[0] if flags.get(name) else "-"
    return text
