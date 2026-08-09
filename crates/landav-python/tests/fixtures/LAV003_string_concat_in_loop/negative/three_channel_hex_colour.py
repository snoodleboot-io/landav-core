"""Format a pixel as a CSS hex colour."""

_RGB_CHANNELS = 3


def hex_colour(pixel):
    """Three iterations, fixed by the constant in the header.

    ``range(_RGB_CHANNELS)`` runs exactly three times whatever the input is, so
    the three concatenations are a constant, not a quadratic accumulation.
    Building a list and joining it would allocate more, not less.
    """
    text = "#"
    for channel in range(_RGB_CHANNELS):
        text += "%02x" % pixel[channel]
    return text
