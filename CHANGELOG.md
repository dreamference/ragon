# Changelog

The full notes of each release are in `specs/DREAMFERENCE_RELEASE_<version>.md`.

## 0.9.6 "Ladon" (2026-11-13)

- The Ragon-Builds-Ragon benchmark (RBRB) is public: README section, chart, spec and commit trailers (`Ragon-Tokens`, `Made-with`).
- A fair engine-versus-SGLang table on the release image: 1.22x at one stream, 1.25x at two, parity at four, 1.13x at 32 forks against an equally configured SGLang (1.76x against its production launch).
- The delegated edit tool refuses diffs that touch the quality gates; 35 small cleanups were made by Ragon through the RBRB routing.
- SWE-bench Verified on the 0.9.5 image: slice 1 28/39, slice 3 69/106, slice 4 69/106, 0 starved.
