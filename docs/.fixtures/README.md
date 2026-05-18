# Doc fixture: sample-repo

Used by `.pre-commit-hooks/regen-doc-snippets.py` as the project against
which the documentation's captured `--help` and command-output `.txt`
snippets are produced.

The `tools/example.py` here mirrors `src/bin/toolr/init_templates/example.py.tmpl`
byte-for-byte; if either changes, regenerate the snippets:

```sh
.pre-commit-hooks/regen-doc-snippets.py
```
