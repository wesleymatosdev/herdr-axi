# Recovery evidence — 2026-09-05

The local OpenCode worker left the repo in a broken state (stubs replacing
working implementation, no `fn main`, cargo check E0601). Its entire dirty
diff and untracked `crates/` tree were preserved verbatim as a commit on a
dedicated branch, NOT lost, NOT merged into master history:

- Branch: `recovery/opencode-wip-20260905`
- Commit: `4ec6b80` ("evidence: opencode worker WIP snapshot ...")
- Parent (original, unmodified HEAD used as behavioral baseline): `e03a428`

To inspect the raw worker diff at any time:

```
git diff e03a428 4ec6b80            # full diff
git diff e03a428 4ec6b80 --stat     # summary
git show 4ec6b80:crates/cli/src/main.rs   # any specific stub file
```

master was restored to `e03a428` (untouched) before real recovery work
began. All finished refactor work lands as new commits on top of
`e03a428`, not on top of the broken WIP.
