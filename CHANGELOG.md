# Changelog

The full notes of each release are in `specs/DREAMFERENCE_RELEASE_<version>.md`.

## 0.9.8 "Illuyanka" (2026-11-20)

- Signal sends one new message per progress step instead of editing a message, so nothing flickers; editing stays an option (`ragon signal delivery edit`).
- No new engine image: the 0.9.7 image stays pinned.
- The platform quality specification: the Windows and Mac ports are judged on the same quality, not the same results, by benchmark bands from N = 5 reference runs.
- SWE-bench Verified reference set on the 0.9.7 image: runs per slice with their spread (the four-slice set averages 239); this set is the band the ports are judged against.
- RBRB week 2026-W47 so far: 8.7 % of generated tokens were Ragon's against a 7 % gate; the chart shows the running week and the target.
- 72 small cleanups (typing, test coverage, constants) were landed by Ragon through the RBRB routing, none reverted.
- The Windows engine port (HIP) passed its decode gate and its first speed lane.

## 0.9.7 "Vritra" (2026-11-17)

- A new engine image (ragon-engine 46ab4259, `ragon-serve@sha256:f4391e2e`) with the fan-out policy: two exact levers on by default, 32 forks x 32K at 287 against 192 tok/s decode span on the 0.9.6 image, greedy output identical on 16 of 16 turns.
- A fair engine-versus-SGLang table on the release image: 1.24x at one stream, 1.23x at two, 1.08x pooled at four warm streams, 1.48x at 32 forks against an equally configured SGLang (2.40x against its production launch).
- RBRB week 2026-W46: 8.0 % of generated development tokens were Ragon's, 0 reverts; the ratchet now tracks the share (last week's minus 2 points, floor 2 %).
- SWE-bench Verified, four-slice set on the 0.9.7 image: 0.9.7: 241 (spread 232-248); 0.9.6: 232-236.
- The Windows engine port (HIP) passed its first gates.

## 0.9.6 "Ladon" (2026-11-13)

- The Ragon-Builds-Ragon benchmark (RBRB) is public: README section, chart, spec and commit trailers (`Ragon-Tokens`, `Made-with`).
- A fair engine-versus-SGLang table on the release image: 1.22x at one stream, 1.25x at two, parity at four, 1.13x at 32 forks against an equally configured SGLang (1.76x against its production launch).
- The delegated edit tool refuses diffs that touch the quality gates; 35 small cleanups were made by Ragon through the RBRB routing.
- SWE-bench Verified on the 0.9.5 image: slice 1 28/39, slice 3 69/106, slice 4 69/106, 0 starved.
