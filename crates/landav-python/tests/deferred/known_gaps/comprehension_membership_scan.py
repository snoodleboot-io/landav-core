"""LAV002 in comprehension form. Genuinely quadratic, and invisible.

Compare `tests/fixtures/LAV002_list_membership_in_loop/positive/
banned_names_list.py`, which is the same computation written with a `for`
statement and which LAV002 reports. This file is silent.

`BANNED` has eight entries deliberately: at seven the scan is bounded and
LAV002 is right to say nothing, so a shorter list would confound the
demonstration with the `MIN_SCANNED_LIST` threshold.
"""

BANNED = ["root", "admin", "daemon", "operator", "backup", "www-data", "nobody", "sync"]


def strip_system_users(users):
    """One list scan per user, exactly as in the statement form."""
    return [user for user in users if user.name in BANNED]
