================
git-bot-feedback
================

|pypi-badge|
|py-ci-badge|
|docs-badge|
|license-badge|

A Python library (written in Rust) designed for CI tools to easily submit feedback on a git server.

Feedback on a git server using this library can be in the form of

- thread comments (for a PR or commit)
- setting output variables for other CI tools to consume
- append a summary comment to a CI workflow run's summary page
- mark the start and end of a group of log statements (in the CI workflow run's
  logs)
- files annotations
- Pull Request reviews
- get a list of changed files in a PR or commit (including line numbers shown in the diff)

This uses async functions to access network resources. Thus, an ``asyncio`` event
loop is required.

Supported git servers
---------------------

This project is designed to easily add support for various git servers.
The following is just a list of git servers that are planned (in order or priority).

- GitHub
- GitLab
- Gitea

  Gitea does not support

  - posting thread comments for commits (push events)
  - programmatically deleting a PR reviews' individual comments, rather we can
    only resolve them (currently). However, deleting an entire PR review is supported.
- BitBucket

Currently, only Github and Gitea are supported.

LGPL license
------------

.. _LGPL-3.0-or-later: https://github.com/2bndy5/git-bot-feedback/blob/main/LICENSE

This project is licensed under `LGPL-3.0-or-later`_.

Since this library ultimately requires write access to
users' projects (to allow posting comments),
it could easily be modified with malicious intent.

By using the `LGPL-3.0-or-later`_ license,
we can offer some assurance and help safeguard end-users' data/privacy
because the following conditions must be met:

- the source code is publicly available
- any redistributed forms must state their modifications (if any)

.. |docs-badge| image:: https://img.shields.io/github/deployments/2bndy5/git-bot-feedback/github-pages?logo=github&label=docs
   :alt: GitHub pages
   :target: https://2bndy5.github.io/git-bot-feedback
.. |license-badge| image:: https://img.shields.io/github/license/2bndy5/git-bot-feedback
   :alt: GitHub License
   :target: https://github.com/2bndy5/git-bot-feedback/blob/main/LICENSE
.. |py-ci-badge| image:: https://github.com/2bndy5/git-bot-feedback/actions/workflows/python.yml/badge.svg
    :alt: Python CI
    :target: https://github.com/2bndy5/git-bot-feedback/actions/workflows/python.yml
.. |pypi-badge| image:: https://img.shields.io/pypi/v/git-bot-feedback
   :alt: PyPI Version
   :target: https://pypi.org/project/git-bot-feedback
