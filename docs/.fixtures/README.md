# Doc fixture: sample-repo

Used by `toolr pre-commit regen-doc-snippets` as the project against
which the documentation's captured `--help` and command-output `.txt`
snippets are produced.

The `tools/example.py` here mirrors `crates/toolr/src/init_templates/example.py.tmpl`
byte-for-byte; if either changes, regenerate the snippets:

```sh
toolr pre-commit regen-doc-snippets
```
