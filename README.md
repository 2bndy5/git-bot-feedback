# Git-bot-feedback

[![rust-ci-badge]][rust-ci-runs] [![docs-rs-badge]][docs-rs-link] [![codecov-badge]][codecov-link] [![crates-io-badge]][crates-io-link]

[codecov-badge]: https://codecov.io/gh/2bndy5/git-bot-feedback/graph/badge.svg?token=T3FRIJ64W0
[codecov-link]: https://app.codecov.io/gh/2bndy5/git-bot-feedback
[rust-ci-badge]: https://github.com/2bndy5/git-bot-feedback/actions/workflows/rust.yml/badge.svg
[rust-ci-runs]: https://github.com/2bndy5/git-bot-feedback/actions/workflows/rust.yml
[crates-io-badge]: https://img.shields.io/crates/v/git-bot-feedback
[crates-io-link]: https://crates.io/crates/git-bot-feedback
[docs-rs-badge]: https://img.shields.io/docsrs/git-bot-feedback
[docs-rs-link]: https://docs.rs/git-bot-feedback

A rust library designed for CI tools to easily submit feedback on a git server.

Feedback on a git server using this library can be in the form of

- thread comments (for a PR or commit)
- setting output variables for other CI tools to consume
- append a summary comment to a CI workflow run's summary page
- mark the start and end of a group of log statements (in the CI workflow run's logs)

More features are planned, like PR reviews and file annotations.

## Optional Features

These [cargo features][dep-features] are optional and disabled by default:

- `file-changes`: ability to list files changed with information like
  which lines have additions or which lines are shown in the diff.

[dep-features]: https://doc.rust-lang.org/cargo/reference/features.html#dependency-features

## Supported git servers

Initially this project os designed to work with GitHub.
But the API is designed to easily add support for other git servers.
The following is just a list of git servers that are planned (in order or priority).

- [x] GitHub
- [ ] GitLab
- [ ] Gitea
- [ ] BitBucket

## GPL license

[GPL-3.0-or-later]: https://choosealicense.com/licenses/gpl-3.0/

This project is licensed under [GPL-3.0-or-later].

Since this library ultimately requires write access to
users' projects (to allow posting comments),
it could easily be modified with malicious intent.

By using the [GPL-3.0-or-later] license,
we can offer some assurance and help safeguard end-users' data/privacy
because the following conditions must be met:

- the source code is publicly available
- any redistributed forms must state their modifications (if any)
- any redistributed forms must use the same [GPL-3.0-or-later] license
