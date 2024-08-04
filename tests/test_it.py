import toolr
import toolr.rust  # type: ignore[import-not-found]


def test_python():
    assert toolr.python_func() == 15


def test_rust():
    assert toolr.rust.rust_func() == 14
