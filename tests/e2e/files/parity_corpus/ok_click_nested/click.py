"""Stands in for click, which the test environment may not install."""


def command(*args, **kwargs):
    return lambda func: func
