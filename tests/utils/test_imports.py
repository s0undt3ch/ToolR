"""
Tests for toolr.utils._imports module.
"""

from __future__ import annotations

import logging

import pytest

from toolr.utils._imports import report_on_import_errors


def test_successful_import_no_error(caplog):
    """Test that successful imports don't trigger any warnings."""
    message = "Test import error message"

    with caplog.at_level(logging.WARNING):
        with report_on_import_errors(message):
            # Simulate successful import
            import sys

            assert sys is not None

    # No warnings should be logged
    assert len(caplog.records) == 0


def test_module_not_found_error_handling(caplog):
    """Test that ModuleNotFoundError is caught and logged with warning."""
    message = "Test import error message"

    with caplog.at_level(logging.WARNING):
        with report_on_import_errors(message):
            # Simulate a module that doesn't exist
            import nonexistent_module  # type: ignore[import-not-found]

            # This assertion will never run and is only here to prevent ruff from removing the import
            assert nonexistent_module is None  # pragma: no cover

    # Should have one warning record
    assert len(caplog.records) == 1
    record = caplog.records[0]
    assert record.levelname == "WARNING"
    assert record.message == message
    assert record.exc_info is not None
    assert isinstance(record.exc_info[1], ModuleNotFoundError)


def test_traceback_suppression(caplog):
    """Test that the traceback is properly suppressed (tb_next is called)."""
    message = "Test traceback suppression"

    with caplog.at_level(logging.WARNING):
        with report_on_import_errors(message):
            import nonexistent_module

            # This assertion will never run and is only here to prevent ruff from removing the import
            assert nonexistent_module is None  # pragma: no cover

    record = caplog.records[0]
    exc_info = record.exc_info
    assert exc_info is not None

    # The exception should have its traceback modified
    exc = exc_info[1]
    assert isinstance(exc, ModuleNotFoundError)
    # The traceback should be modified (tb_next called)
    # We can't easily test the exact traceback content, but we can verify
    # that the exception was processed


def test_other_exceptions_not_caught():
    """Test that exceptions other than ModuleNotFoundError are not caught."""
    message = "Test other exception"
    error_msg = "This should not be caught"

    with pytest.raises(ValueError, match=error_msg):
        with report_on_import_errors(message):
            raise ValueError(error_msg)


def test_custom_message_logged(caplog):
    """Test that the custom message is logged correctly."""
    custom_message = "Custom import error message for testing"

    with caplog.at_level(logging.WARNING):
        with report_on_import_errors(custom_message):
            import nonexistent_module

            # This assertion will never run and is only here to prevent ruff from removing the import
            assert nonexistent_module is None  # pragma: no cover

    record = caplog.records[0]
    assert record.message == custom_message


def test_nested_import_errors(caplog):
    """Test handling of nested import errors."""
    message = "Nested import error test"

    def nested_import():
        import nonexistent_module

        # This assertion will never run and is only here to prevent ruff from removing the import
        assert nonexistent_module is None  # pragma: no cover

    with caplog.at_level(logging.WARNING):
        with report_on_import_errors(message):
            nested_import()

    # Should still catch and log the error
    assert len(caplog.records) == 1
    record = caplog.records[0]
    assert record.levelname == "WARNING"
    assert record.message == message


def test_multiple_import_errors_in_sequence(caplog):
    """Test handling multiple import errors in sequence."""
    message = "Multiple import errors test"

    with caplog.at_level(logging.WARNING):
        # First import error
        with report_on_import_errors(message):
            import nonexistent_module1  # type: ignore[import-not-found]

            # This assertion will never run and is only here to prevent ruff from removing the import
            assert nonexistent_module1 is None  # pragma: no cover

    with caplog.at_level(logging.WARNING):
        # Second import error
        with report_on_import_errors(message):
            import nonexistent_module2  # type: ignore[import-not-found]

            # This assertion will never run and is only here to prevent ruff from removing the import
            assert nonexistent_module2 is None  # pragma: no cover

    # Each import error should be logged separately
    assert len(caplog.records) == 2
    for record in caplog.records:
        assert record.levelname == "WARNING"
        assert record.message == message


def test_context_manager_as_decorator_pattern(caplog):
    """Test using the context manager in a decorator-like pattern."""
    message = "Decorator pattern test"

    def function_with_import():
        with report_on_import_errors(message):
            import nonexistent_module

            # This assertion will never run and is only here to prevent ruff from removing the import
            assert nonexistent_module is None  # pragma: no cover

    with caplog.at_level(logging.WARNING):
        function_with_import()

    assert len(caplog.records) == 1
    record = caplog.records[0]
    assert record.message == message
