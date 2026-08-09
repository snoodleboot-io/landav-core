"""Collect the settings that exist for a host list."""


def collect_configured(hosts, settings):
    """The handler *is* the filter, so the raise rate is the miss rate.

    ``settings.get(host)`` is never slower and never raises.
    """
    chosen = []
    for host in hosts:
        try:  # LANDAV: LAV010 anchor=try:
            chosen.append(settings[host])
        except KeyError:
            continue
    return chosen
