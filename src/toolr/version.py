# pylint: skip-file

try:
    from toolr._version import __version__  # type: ignore[import-not-found]
except ImportError:
    try:
        import importlib.metadata

        __version__ = importlib.metadata.version("toolr")
    except ImportError:
        # Fallback if anything else fails
        __version__ = "0.0.0.unreleased"
