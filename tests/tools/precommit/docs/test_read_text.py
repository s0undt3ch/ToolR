"""Tests for `_read_text`, which normalises a captured stream to ``str``."""

from __future__ import annotations

import io

import pytest

from tools.precommit.docs import _read_text


@pytest.mark.parametrize("stream", [io.StringIO("héllo"), io.BytesIO("héllo".encode())])
def test_reads_str_and_bytes_streams_as_text(stream: io.StringIO | io.BytesIO) -> None:
    assert _read_text(stream) == "héllo"
