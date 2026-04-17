# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
<!-- markdownlint-disable MD024 -->

## [0.5.1] - 2026-04-17

### <!-- 4 --> 🛠️ Fixed

- Write only 1 LF for GH output vars by @2bndy5 in [`7add27b`](https://github.com/2bndy5/git-bot-feedback/commit/7add27b0637d65e2741e05eb55e3e3e712f46236)

[0.5.1]: https://github.com/2bndy5/git-bot-feedback/compare/v0.5.0...v0.5.1

Full commit diff: [`v0.5.0...v0.5.1`][0.5.1]

## [0.5.0] - 2026-04-17

### <!-- 4 --> 🛠️ Fixed

- Use stricter linting rules by @2bndy5 in [#63](https://github.com/2bndy5/git-bot-feedback/pull/63)

### <!-- 6 --> 📦 Dependency updates

- Bump version to v0.5.0 by @2bndy5 in [`83121f2`](https://github.com/2bndy5/git-bot-feedback/commit/83121f2984388b4013dc3c30b849659d5e2533d9)

[0.5.0]: https://github.com/2bndy5/git-bot-feedback/compare/v0.4.0...v0.5.0

Full commit diff: [`v0.4.0...v0.5.0`][0.5.0]

## [0.4.0] - 2026-04-17

### <!-- 1 --> 🚀 Added

- Expose `debug_enabled` and `event_name` by @2bndy5 in [#60](https://github.com/2bndy5/git-bot-feedback/pull/60)
- Implement Display for LinesChangedOnly by @2bndy5 in [#59](https://github.com/2bndy5/git-bot-feedback/pull/59)
- Add `init_client()` and `LocalClient` by @2bndy5 in [#61](https://github.com/2bndy5/git-bot-feedback/pull/61)
- Allow custom user-agent header value by @2bndy5 in [#62](https://github.com/2bndy5/git-bot-feedback/pull/62)

### <!-- 10 --> 💥 Breaking Changes

- Rename `FileFilter::is_not_ignored()` to `is_qualified()` by @2bndy5 in [#57](https://github.com/2bndy5/git-bot-feedback/pull/57)

### <!-- 4 --> 🛠️ Fixed

- Remove `async` from non-async functions by @2bndy5 in [#58](https://github.com/2bndy5/git-bot-feedback/pull/58)

### <!-- 6 --> 📦 Dependency updates

- Bump version to v0.4.0 by @2bndy5 in [`d4406ff`](https://github.com/2bndy5/git-bot-feedback/commit/d4406ff45f9bc3a83580747e466cd157fe393efe)

### <!-- 9 --> 🗨️ Changed

- Exclude CHANGELOG.md in spell-check by @2bndy5 in [`7498dbd`](https://github.com/2bndy5/git-bot-feedback/commit/7498dbd8af06c7fcac34f4a34de246862efa4ae0)
- Checkout repo before uploading coverage report by @2bndy5 in [`b13220a`](https://github.com/2bndy5/git-bot-feedback/commit/b13220ac202648ed8e8925c01af0ad876b2aa7ed)
- Use GH token for creating unreleased changelog by @2bndy5 in [`27bb437`](https://github.com/2bndy5/git-bot-feedback/commit/27bb437fc244381cf6e58fafa0a46b77ceb38d53)
- Update locked transitive dependencies by @2bndy5 in [`497d958`](https://github.com/2bndy5/git-bot-feedback/commit/497d95873a98960794958d9dfc59d99c26762c31)

[0.4.0]: https://github.com/2bndy5/git-bot-feedback/compare/v0.3.0...v0.4.0

Full commit diff: [`v0.3.0...v0.4.0`][0.4.0]

## [0.3.0] - 2026-04-15

### <!-- 1 --> 🚀 Added

- Allow custom base for local diff by @2bndy5 in [#30](https://github.com/2bndy5/git-bot-feedback/pull/30)
- Propagate more errors to user for adequate handling by @2bndy5 in [#31](https://github.com/2bndy5/git-bot-feedback/pull/31)
- Use helper functions to reduce error handling code by @2bndy5 in [#33](https://github.com/2bndy5/git-bot-feedback/pull/33)
- Comprehensive output variable validation by @2bndy5 in [#35](https://github.com/2bndy5/git-bot-feedback/pull/35)
- Support file annotations by @2bndy5 in [#37](https://github.com/2bndy5/git-bot-feedback/pull/37)
- Support PR reviews by @2bndy5 in [#34](https://github.com/2bndy5/git-bot-feedback/pull/34)

### <!-- 4 --> 🛠️ Fixed

- Use synchronous `std::{fs, process::Command}` by @2bndy5 in [#29](https://github.com/2bndy5/git-bot-feedback/pull/29)
- Make `RestApiClient` a dyn-compatible async trait by @2bndy5 in [#36](https://github.com/2bndy5/git-bot-feedback/pull/36)

### <!-- 6 --> 📦 Dependency updates

- Bump bytes from 1.10.1 to 1.11.1 by @dependabot[bot] in [#27](https://github.com/2bndy5/git-bot-feedback/pull/27)
- Bump git-cliff in /.github in the pip group by @dependabot[bot] in [#23](https://github.com/2bndy5/git-bot-feedback/pull/23)
- Bump the actions group across 1 directory with 8 updates by @dependabot[bot] in [#21](https://github.com/2bndy5/git-bot-feedback/pull/21)
- Bump the cargo group across 1 directory with 10 updates by @dependabot[bot] in [#28](https://github.com/2bndy5/git-bot-feedback/pull/28)
- Bump the cargo group across 1 directory with 4 updates by @dependabot[bot] in [#40](https://github.com/2bndy5/git-bot-feedback/pull/40)
- Bump actions/setup-node from v6 in the actions group by @dependabot[bot] in [#43](https://github.com/2bndy5/git-bot-feedback/pull/43)
- Bump quinn-proto from 0.11.13 to 0.11.14 by @dependabot[bot] in [#41](https://github.com/2bndy5/git-bot-feedback/pull/41)
- Bump rand from 0.9.2 to 0.9.4 by @dependabot[bot] in [#49](https://github.com/2bndy5/git-bot-feedback/pull/49)
- Bump the actions group across 1 directory with 5 updates by @dependabot[bot] in [#48](https://github.com/2bndy5/git-bot-feedback/pull/48)
- Bump rustls-webpki from 0.103.9 to 0.103.12 by @dependabot[bot] in [#50](https://github.com/2bndy5/git-bot-feedback/pull/50)
- Bump tokio from 1.50.0 to 1.51.0 in the cargo group by @dependabot[bot] in [#47](https://github.com/2bndy5/git-bot-feedback/pull/47)
- Bump tokio to v1.52.0 by @2bndy5 in [`b0ef1d5`](https://github.com/2bndy5/git-bot-feedback/commit/b0ef1d5668218c4c445048583e24a78fccf7664a)
- Bump version to v0.3.0 by @2bndy5 in [`c8e8341`](https://github.com/2bndy5/git-bot-feedback/commit/c8e834160052393dfe626027e5ef997033047f16)

### <!-- 9 --> 🗨️ Changed

- Add zizmor config by @2bndy5 in [`9f5b331`](https://github.com/2bndy5/git-bot-feedback/commit/9f5b3318eaa186b3250cf599bfbfecbb743cb755)
- Satisfy zizmor audit violations by @2bndy5 in [#42](https://github.com/2bndy5/git-bot-feedback/pull/42)

[0.3.0]: https://github.com/2bndy5/git-bot-feedback/compare/v0.2.0...v0.3.0

Full commit diff: [`v0.2.0...v0.3.0`][0.3.0]

## [0.2.0] - 2025-10-25

### <!-- 1 --> 🚀 Added

- List file changes by @2bndy5 in [#11](https://github.com/2bndy5/git-bot-feedback/pull/11)

### <!-- 6 --> 📦 Dependency updates

- Replace deprecated nushell syntax by @2bndy5 in [`921c043`](https://github.com/2bndy5/git-bot-feedback/commit/921c0439a24f8fb387fd816271b67e8be48a2925)
- Bump slab from 0.4.10 to 0.4.11 by @dependabot[bot] in [#4](https://github.com/2bndy5/git-bot-feedback/pull/4)
- Bump the actions group across 1 directory with 2 updates by @dependabot[bot] in [#6](https://github.com/2bndy5/git-bot-feedback/pull/6)
- Bump the cargo group across 1 directory with 9 updates by @dependabot[bot] in [#10](https://github.com/2bndy5/git-bot-feedback/pull/10)
- Bump git-cliff from 2.10.0 to 2.10.1 in /.github in the pip group by @dependabot[bot] in [#12](https://github.com/2bndy5/git-bot-feedback/pull/12)
- Bump the actions group with 3 updates by @dependabot[bot] in [#13](https://github.com/2bndy5/git-bot-feedback/pull/13)
- Bump version to v0.2.0 by @2bndy5 in [`bfbedaf`](https://github.com/2bndy5/git-bot-feedback/commit/bfbedaf5953ef401268ca832c65f19f796ec6cf8)

### <!-- 9 --> 🗨️ Changed

- Regenerate CHANGELOG by @2bndy5 in [`43b62a8`](https://github.com/2bndy5/git-bot-feedback/commit/43b62a876847409358146d65c7b309e09996583f)
- Fix bump-n-release script by @2bndy5 in [`b7c8ee3`](https://github.com/2bndy5/git-bot-feedback/commit/b7c8ee3fd68816c3dc2ba6583a1dd832122a2936)

[0.2.0]: https://github.com/2bndy5/git-bot-feedback/compare/v0.1.4...v0.2.0

Full commit diff: [`v0.1.4...v0.2.0`][0.2.0]

## [0.1.4] - 2025-08-18

### <!-- 6 --> 📦 Dependency updates

- Check in Cargo.lock by @2bndy5 in [`8642a56`](https://github.com/2bndy5/git-bot-feedback/commit/8642a561eeefc241e61c56cac29ade1dfab1fb1b)
- Bump the cargo group with 3 updates by @dependabot[bot] in [#2](https://github.com/2bndy5/git-bot-feedback/pull/2)
- Update locked transitive dependencies by @2bndy5 in [`db158c0`](https://github.com/2bndy5/git-bot-feedback/commit/db158c0ce96163471a569299be2c5ee8a13c66a6)
- Bump version to v0.1.4 by @2bndy5 in [`19c6330`](https://github.com/2bndy5/git-bot-feedback/commit/19c6330e8c4aa0e4ee18482b761277bd294bb6f3)

### <!-- 9 --> 🗨️ Changed

- Update dependabot config by @2bndy5 in [`f5b2097`](https://github.com/2bndy5/git-bot-feedback/commit/f5b2097d46b101ff133323fe7e7d81d8a0c6e1aa)

[0.1.4]: https://github.com/2bndy5/git-bot-feedback/compare/v0.1.3...v0.1.4

Full commit diff: [`v0.1.3...v0.1.4`][0.1.4]

## New Contributors

- @dependabot[bot] made their first contribution in [#2](https://github.com/2bndy5/git-bot-feedback/pull/2)

## [0.1.3] - 2025-07-19

### <!-- 6 --> 📦 Dependency updates

- Bump version to v0.1.3 by @2bndy5 in [`c5e4d9b`](https://github.com/2bndy5/git-bot-feedback/commit/c5e4d9b9ce474d26e6a92286c1b58207351cd153)

### <!-- 9 --> 🗨️ Changed

- Bump serde_json from 1.0.140 to 1.0.141 by @2bndy5 in [`e831cd8`](https://github.com/2bndy5/git-bot-feedback/commit/e831cd8b937620eb804f4efad02f683d0317eeb6)

[0.1.3]: https://github.com/2bndy5/git-bot-feedback/compare/v0.1.2...v0.1.3

Full commit diff: [`v0.1.2...v0.1.3`][0.1.3]

## [0.1.2] - 2025-07-16

### <!-- 6 --> 📦 Dependency updates

- Detect CI context in build-n-release.nu script by @2bndy5 in [`ae77080`](https://github.com/2bndy5/git-bot-feedback/commit/ae77080563f20f0374c4692d79d2096abbe14b66)
- Bump version to v0.1.2 by @2bndy5 in [`9c6cef4`](https://github.com/2bndy5/git-bot-feedback/commit/9c6cef41be1c2780b95cc18277bb9e180d78886f)

### <!-- 8 --> 📝 Documentation

- Add badges to README by @2bndy5 in [`ea7212d`](https://github.com/2bndy5/git-bot-feedback/commit/ea7212d9a248260a02a531394115ab58fadfe67f)

[0.1.2]: https://github.com/2bndy5/git-bot-feedback/compare/v0.1.1...v0.1.2

Full commit diff: [`v0.1.1...v0.1.2`][0.1.2]

## [0.1.1] - 2025-07-16

### <!-- 1 --> 🚀 Added

- Implement GitHub client by @2bndy5 in [#1](https://github.com/2bndy5/git-bot-feedback/pull/1)

### <!-- 6 --> 📦 Dependency updates

- Adjust bump-n-release.nu script by @2bndy5 in [`ccaa011`](https://github.com/2bndy5/git-bot-feedback/commit/ccaa0113b94e4c103e4fa258e249f2b932070ef7)
- Bump version to v0.1.1 by @2bndy5 in [`6a508c0`](https://github.com/2bndy5/git-bot-feedback/commit/6a508c0f58c2e97684f369d9fecc3349203bfd07)

### <!-- 9 --> 🗨️ Changed

- Initial commit by @2bndy5 in [`4d46f96`](https://github.com/2bndy5/git-bot-feedback/commit/4d46f96eddc6f512bb7bf3600d3f9d5490ef004f)

[0.1.1]: https://github.com/2bndy5/git-bot-feedback/compare/4d46f96eddc6f512bb7bf3600d3f9d5490ef004f...v0.1.1

Full commit diff: [`4d46f96...v0.1.1`][0.1.1]

## New Contributors

- @2bndy5 made their first contribution


<!-- generated by git-cliff -->
