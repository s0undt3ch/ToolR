# Release notes

Each file here is one queued release note. On release, `toolr release-notes render` joins them into the
`### Notes` section of the GitHub release body and `CHANGELOG.md`, and `toolr release-notes clear` empties
the directory. This README is never treated as a note.

## Adding a note

```sh
toolr release-notes add --slug my-change --text "What changed, and what a user needs to do about it."
```

Without `--slug`, the slug comes from the current branch name. Without `--text`, an empty file is created
for you to edit. `--append` adds to an existing note with the same slug. The command stages the file.

Notes are named `<YYYY-MM-DD>.<HH-MM-SS>.<slug>.md`, using the UTC creation time. The slug is lowercase
letters and digits separated by single hyphens. The timestamp keeps notes in the order they were written.

## Writing a note

- Write plain prose, lists and code blocks. Do not use headings: `render` places the notes under one.
- Start a note with `**Breaking:**` if it describes a breaking change. Those notes are rendered first.
- One note per change. Several paragraphs and lists are fine inside one note.

## The PR gate

A PR that changes user-visible behaviour needs a note. `toolr release-notes check --base <branch>` fails
when the PR adds none. Put `Release-Notes: skip` in a commit message trailer when no note is needed;
`Changelog: skip` is honoured too. A PR that only adds notes should use a `docs(release-notes): ...`
commit.

## After a parent PR merges

In a stack, restack and push the child branch. Do not re-run the old CI run: it checked the branch against
the previous base.
