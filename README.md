# Git-bot-feedback

[![rust-ci-badge]][rust-ci-runs]
[![docs-rs-badge]][docs-rs-link]
[![codecov-badge]][codecov-link]
[![crates-io-badge]][crates-io-link]
[![GitHub License][license-badge]][license-link]

A rust library designed for CI tools to easily submit feedback on a git server.

Feedback on a git server using this library can be in the form of

- thread comments (for a PR or commit)
- setting output variables for other CI tools to consume
- append a summary comment to a CI workflow run's summary page
- mark the start and end of a group of log statements (in the CI workflow run's
  logs)
- files annotations
- Pull Request reviews

## Optional Features

These [cargo features][dep-features] are optional and disabled by default:

- `file-changes`: ability to list files changed with information like
  which lines have additions or which lines are shown in the diff.

### TLS Backend

A TLS backend is explicitly not set by this crate. This is intended to allow library
consumers to choose the TLS backend of their choice; see [reqwest's features][reqwest-docs].

## Supported git servers

This project is designed to easily add support for various git servers.
The following is just a list of git servers that are planned (in order or priority).

- [x] GitHub
- [ ] GitLab
- [x] Gitea

  Gitea does not support

  - posting thread comments for commits (push events)
  - programmatically deleting a PR reviews' individual comments,
    rather we can only resolve them (currently).
    However, deleting an entire PR review is supported.
- [ ] BitBucket

### Optional support

Each supported implementation of the above git servers can be controlled via
[cargo features][dep-features]. They are enabled by default.

- `github` enables support of GitHub implementation
- `gitea` enables support of Gitea implementation

## LGPL license

This project is licensed under [LGPL-3.0-or-later].

Since this library ultimately requires write access to
users' projects (to allow posting comments),
it could easily be modified with malicious intent.

By using the [LGPL-3.0-or-later] license,
we can offer some assurance and help safeguard end-users' data/privacy
because the following conditions must be met:

- the source code is publicly available
- any redistributed forms must state their modifications (if any)

[codecov-badge]: https://codecov.io/gh/2bndy5/git-bot-feedback/graph/badge.svg?token=T3FRIJ64W0
[codecov-link]: https://app.codecov.io/gh/2bndy5/git-bot-feedback
[rust-ci-badge]: https://github.com/2bndy5/git-bot-feedback/actions/workflows/rust.yml/badge.svg
[rust-ci-runs]: https://github.com/2bndy5/git-bot-feedback/actions/workflows/rust.yml
[crates-io-badge]: https://img.shields.io/crates/v/git-bot-feedback
[crates-io-link]: https://crates.io/crates/git-bot-feedback
[docs-rs-badge]: https://img.shields.io/docsrs/git-bot-feedback?logo=docsdotrs
[docs-rs-link]: https://docs.rs/git-bot-feedback
[dep-features]: https://doc.rust-lang.org/cargo/reference/features.html#dependency-features
[reqwest-docs]: https://docs.rs/reqwest/latest/reqwest/#optional-features
[LGPL-3.0-or-later]: https://github.com/2bndy5/git-bot-feedback/blob/main/LICENSE
[license-badge]: https://img.shields.io/github/license/2bndy5/git-bot-feedback
[license-link]: https://github.com/2bndy5/git-bot-feedback/blob/main/LICENSE
