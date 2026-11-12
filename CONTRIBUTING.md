# Contributing to MistTerm

Thanks for your interest! We welcome bug reports, feature requests, and pull requests.

## Reporting Issues

**In the app:** **Help → Report an Issue** (帮助 → 问题反馈) opens GitHub with the bug report template and your version in the title.

**On GitHub:** [New issue](https://github.com/mistlab-dev/MistTerm/issues/new/choose) — pick **Bug Report** or **Feature Request**.

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

### Review Terminology

Use one consistent set of terms in review comments and PR descriptions, so
"can this merge?" never depends on the reader's vocabulary. Every finding is
classified by **whether it blocks merging**:

| Term (zh) | English | Meaning | Action |
| --- | --- | --- | --- |
| 阻塞项 | blocker / must-fix | Wrong behavior, regression, or a violated product gate | Must be fixed before merge; review state `CHANGES_REQUESTED` |
| 非阻塞建议 | non-blocking suggestion | Correct as-is, but could be improved | Does **not** block merge; review state `COMMENTED`; fix now or later |
| nit | nit | Pure style, naming, wording, formatting | Optional; never blocks |
| follow-up | follow-up | Out of scope for this PR | Record as a separate issue/PR; do not bundle |

The table has **two levels**, not four parallel categories: 阻塞项 / blocker is
the only blocking category; 非阻塞建议 / non-blocking suggestion is the umbrella
term for everything non-blocking, and `nit` / `follow-up` are two specialised
subcategories of it — they say *why* a finding is non-blocking, not a third or
fourth verdict. Every finding is either a blocker or a non-blocking suggestion,
optionally tagged `nit` / `follow-up`.

Rules:

- Label each finding explicitly, by level: first blocking vs non-blocking,
  then (if non-blocking) the optional `nit` / `follow-up` tag.
- "Non-blocking" means exactly that: the PR may merge with it open. If a change
  is required, mark it 阻塞项 / blocker.
- GitHub allows only one review state per review. If a review contains **any**
  blocker, use `CHANGES_REQUESTED` even when the same review also carries
  non-blocking suggestions. Only-non-blocking reviews use `COMMENTED` (or
  `APPROVED` if the reviewer is approving).
- Avoid ad-hoc shorthand (e.g. 软注, 小问题, 顺便看一眼) in place of the terms
  above — they read differently to different people. Historical reviews used
  软注 for what is now 非阻塞建议; use the table term going forward.
- When reporting status, keep the two levels separate and give an explicit
  breakdown: "N blockers; M non-blocking suggestions total (X nit, Y follow-up,
  rest general)". **M is the total of all non-blocking findings**, the
  parenthetical is a breakdown (X + Y + general = M), not extra counts on top.
  Never write "M non-blocking suggestions (X nit, Y follow-up)" without the
  "total / breakdown" wording — that reads as if nit / follow-up were outside M.

中文评审请直接写「阻塞项」「非阻塞建议」「nit」「follow-up」，不要另造缩写。

### Branches

- Base your PR on `main`
- Use a descriptive branch name: `fix/reconnect-crash`, `feat/snippet-search`

## Releases

**All releases must be cut from `main`.** Do not tag feature branches.

1. Merge your changes into `main` (via PR or direct merge after review).
2. Bump `version` in `Cargo.toml` and `Info.plist` on `main`.
3. Commit, e.g. `chore: release v0.2.x`.
4. Create an annotated tag on **`main`**: `git tag -a v0.2.x -m "v0.2.x"`.
5. Push `main` and the tag: `git push origin main && git push origin v0.2.x`.

Pushing a `v*` tag triggers [Build & Test](.github/workflows/build.yml), which builds platform artifacts and publishes the GitHub Release. CI fails if the tagged commit is not on `origin/main`.

## Code Style

- Follow standard Rust conventions (`cargo fmt`)
- Resolve all clippy warnings (`cargo clippy`)
- Keep public API docs up to date

## Third-Party Licenses

`resources/THIRD_PARTY_LICENSES.txt` lists every bundled dependency and its license text, and is shown in the app under **About → Open-source licenses**. After changing dependencies (`Cargo.toml` / `Cargo.lock`), regenerate and commit it:

```bash
python3 scripts/generate-third-party-licenses.py
```

CI runs the same script with `--check` and fails if the file is out of date. Only add dependencies under permissive or weak-copyleft licenses (MIT, Apache-2.0, BSD, ISC, Zlib, MPL-2.0, …); discuss GPL-family dependencies in an issue first.

## Contributor License Terms

MistTerm is published under the [GNU Affero General Public License v3.0 or later](LICENSE) (AGPL-3.0-or-later). The maintainer also distributes MistTerm through channels whose terms are not compatible with the AGPL (for example the Mac App Store and other app stores), and may offer it under other licenses in the future.

By submitting a contribution (code, documentation, assets, or other material) through a pull request, patch, or any other means, you agree that:

1. **Original work.** The contribution is your own original work, or you have the right to submit it under these terms, and it does not knowingly infringe anyone else's rights.
2. **Open-source license.** Your contribution is licensed to everyone under AGPL-3.0-or-later, the same as the rest of the project.
3. **Additional grant to the maintainer.** You also grant the MistTerm maintainer (Tian and any successor maintainer of the project) a perpetual, worldwide, non-exclusive, royalty-free, irrevocable license to use, modify, sublicense, and distribute your contribution under any license terms, including proprietary and app-store distribution.
4. **Patents.** You grant the same parties a perpetual, worldwide, royalty-free, irrevocable patent license for any of your patent claims that are necessarily infringed by your contribution.
5. **You keep your copyright.** These terms do not transfer ownership of your contribution; you remain free to use it in any other way.

If you cannot agree to these terms (for example because your employer owns your work), please say so in the pull request before it is merged.

## 贡献者授权条款(中文摘要)

MistTerm 以 AGPL-3.0-or-later 开源，同时维护者会通过与 AGPL 不兼容的渠道分发(如 Mac App Store)。提交 PR / 补丁即表示你同意：贡献为你本人原创或你有权提交；贡献以 AGPL-3.0-or-later 向所有人开放；并额外授予维护者(Tian 及项目后续维护者)永久、全球、非独占、免费、不可撤销的许可，可在任意许可条款下(包括闭源与应用商店分发)使用、修改、再许可和分发该贡献，以及相应的专利许可。版权仍归你本人所有。以英文条款为准；如无法同意，请在合并前于 PR 中说明。
