from __future__ import annotations

import re

import pytest
import yaml

from tools.ci import REPO_ROOT


def _hook_pattern() -> re.Pattern[str]:
    config = yaml.safe_load((REPO_ROOT / ".pre-commit-config.yaml").read_text())
    hooks = [hook for repo in config["repos"] for hook in repo.get("hooks", [])]
    (hook,) = [hook for hook in hooks if hook["id"] == "no-inline-test-modules"]
    assert hook["language"] == "pygrep"
    assert "--multiline" in hook["args"]
    # pygrep's --multiline searches the whole file with re.MULTILINE.
    return re.compile(hook["entry"], re.MULTILINE)


@pytest.mark.parametrize(
    "source",
    [
        "#[cfg(test)]\nmod tests {",
        "#[cfg(test)] pub(crate) mod tests {",
        "#[cfg(test)]\nmod tests\n{",
        "#[cfg(test)]\n#[cfg(unix)]\nmod tests {",
        "#[cfg(unix)]\n#[cfg(test)]\nmod tests {",
        "#[cfg(all(test, unix))]\nmod tests {",
        "#[cfg(all(unix, test))]\nmod tests {",
        '#[cfg(any(test, feature = "x"))]\nmod tests {',
        "#[cfg(test)]\n// why\nmod tests {",
        "#[cfg(test)]\n/// doc\nmod tests {",
        "#[cfg(test)]\n/* why */\nmod tests {",
    ],
)
def test_rejects_inline_test_module(source: str) -> None:
    assert _hook_pattern().search(source)


@pytest.mark.parametrize(
    "source",
    [
        "#[cfg(test)]\nmod tests;",
        "#[cfg(all(test, unix))]\nmod tests;",
        "#[cfg(test)]\nfn helper() {}",
        "#[cfg(any(windows, test))]\nfn helper() {}",
        "#[cfg(not(test))]\nmod real {",
        "#[cfg(testing)]\nmod real {",
    ],
)
def test_allows_out_of_line_modules_and_non_test_code(source: str) -> None:
    assert not _hook_pattern().search(source)
