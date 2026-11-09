# Contributing

## Build and test

Requires macOS 14+, a Rust toolchain and the Xcode command line tools.

```bash
cargo build --release
cargo test
./macos/bundle.sh          # builds target/release/Notchpad.app
```

## Workflow

- Open an issue first for anything larger than a small fix.
- One branch per change, cut from `main`.
- Keep diffs small and focused; do not mix refactors with behavior changes.
- Add or update tests for behavior you change.

## Commits

Use [Conventional Commits](https://www.conventionalcommits.org/), lowercase, subject at most 72 characters:

```
fix(quick-add): accept 12am as midnight
docs: clarify reminders sync
```

## Security

Never commit tokens, API keys or credentials. Report vulnerabilities as described in [SECURITY.md](SECURITY.md).
