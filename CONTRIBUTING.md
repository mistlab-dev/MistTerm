# Contributing to MistTerm

Thanks for your interest! We welcome bug reports, feature requests, and pull requests.

## Reporting Issues

**Bug reports** should include:

- OS and MistTerm version (`Help → About` or check window title)
- Steps to reproduce
- Expected vs actual behavior
- Screenshots / terminal output if relevant

**Feature requests** should describe:

- The problem you're trying to solve
- Your proposed solution (if you have one)

Feel free to write in Chinese or English.

## Pull Requests

### Before You Start

- Check existing issues and PRs to avoid duplicates
- For significant changes, open an issue first to discuss the approach

### Development Setup

```bash
git clone https://github.com/mistlab-dev/MistTerm.git
cd MistTerm
cargo build
cargo test
```

Requirements:
- Rust 1.75+ (stable)
- Platform-specific: see build status for supported targets

### Commit Style

- Use clear, descriptive commit messages
- Prefix with conventional tags when possible:
  - `feat:` new feature
  - `fix:` bug fix
  - `docs:` documentation
  - `refactor:` code restructuring
  - `test:` tests
  - `chore:` build, CI, etc.

Example: `feat: add scrollback buffer search`

### PR Guidelines

- One logical change per PR — don't bundle unrelated fixes
- Keep the diff focused and reviewable
- Add tests for new functionality
- Make sure `cargo test` and `cargo clippy` pass
- If changing UI text, consider both English and Chinese

### Branches

- Base your PR on `main`
- Use a descriptive branch name: `fix/reconnect-crash`, `feat/snippet-search`

## Code Style

- Follow standard Rust conventions (`cargo fmt`)
- Resolve all clippy warnings (`cargo clippy`)
- Keep public API docs up to date

## License

By contributing, you agree that your code will be licensed under the [MIT License](LICENSE).
