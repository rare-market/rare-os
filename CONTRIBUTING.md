# Contributing to Rare OS

Thanks for helping. This page covers how to propose a change and how commits are written.

## Before you start

- For a bug, open an issue with the steps to reproduce it, what you expected and what happened. Include the
  launcher version from Settings > About and the device.
- For a larger change or a new feature, open an issue first so the approach can be agreed before you write code.
- Security problems go to the address in [SECURITY.md](SECURITY.md), not to a public issue.

## Development

- Launcher: `cargo build --manifest-path launcher/native/Cargo.toml` and
  `cargo test --manifest-path launcher/crates/Cargo.toml`. CI runs the same tests on every pull request.
- OS image: `podman build -f image/Containerfile -t localhost/rare-os:dev .`, then `image/vm/test.sh` to boot it in
  a virtual machine.
- Keep changes focused: one topic per pull request, with tests for new logic.
- Match the style of the code around your change. Run `cargo fmt` on Rust code.

## Commit messages

Commits follow [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<optional scope>): <summary>
```

Types:

| Type | Use it for |
|---|---|
| `feat` | A new feature |
| `fix` | A bug fix |
| `ui` | A visual or layout change with no new behaviour |
| `perf` | A performance improvement |
| `refactor` | A code change that neither fixes a bug nor adds a feature |
| `docs` | Documentation only |
| `build` | The build system, dependencies or the OS image build |
| `ci` | CI configuration |
| `chore` | Maintenance that fits none of the above |
| `test` | Adding or fixing tests |

The scope is optional and names the area, for example `launcher`, `wallet`, `image`, `emu` or `market`.

Examples:

```
feat(launcher): show download progress on the library tile
fix(wallet): keep the PIN pad focused after a wrong PIN
ui(settings): align the switch column
build(image): update the base image to the latest stable
```

Write the summary in the imperative, in lower case, without a full stop, and keep it under 72 characters. Put
details in the body when the change needs explaining. Mark a breaking change with `!` after the type
(`feat(api)!: ...`) and describe it in the body.

## Pull requests

- Fill in the pull request template.
- Make sure CI passes.
- By contributing you agree that your contribution is licensed under the [Apache License 2.0](LICENSE).
