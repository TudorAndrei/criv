# Hyperframes composition brief: criv

## Objective

Create a 25 second launch-style brag video for criv.

## Output

- Composition directory: `brag-output/composition/`
- Rendered video: `brag-output/brag.mp4`
- Format: landscape, 1920x1080
- Duration: 25 seconds

## Source material

- Project root: `/Users/tudor/cave/criv`
- Primary files read: `site/index.html`, `site/style.css`, `README.md`, `Cargo.toml`, and `docs/architecture/*.c4`
- Product name: criv
- Strongest claim: Keep repository documentation connected to the code it describes.
- Key UI moment: a real terminal check and coverage query, followed by a real ADR supersession timeline
- Copy that must appear verbatim:
  - `criv check`
  - `criv check: ok`
  - `Keep repository documentation connected to the code it describes.`
  - `It has no server and needs no network access.`

## Creative direction

- Tone preset: cinematic
- Creative direction: restrained systems trailer for a local developer tool
- Interpretation: large monospace type, black and soft-white palette, careful holds, and controlled structural motion
- Angle: show documentation drift as a broken physical link, then prove that criv checks source coverage and keeps an append-only ADR history
- Hook: `Documentation drifts.` with a document and source file moving apart
- Outro: the criv wordmark and site tagline
- Avoid generic product claims, abstract filler, neon colors, and unrelated visual redesign

## Visual identity

- Background: `#101010`
- Text: `#d8d8d8`
- Accent: `#d8d8d8`
- Display font: `ui-monospace, SFMono-Regular, Menlo, Consolas, monospace`
- Body font: `ui-monospace, SFMono-Regular, Menlo, Consolas, monospace`
- Visual references: borderless site layout, terminal blocks, and a vertical branch history

## Storyboard

Use `brag-output/brag-plan.md` as the creative contract.

1. Drift, 4.4 seconds: note and source separate, connector breaks.
2. Check, 4.9 seconds: terminal types `criv check` and returns `ok`.
3. Proof, 3.81 seconds: current repository coverage.
4. ADR history, 6.55 seconds: ADR-0038 through ADR-0059, with superseded and accepted states.
5. Connected, 5.34 seconds: criv prompt mark, site tagline, and local-only footer.

## Audio

- Audio role: cinematic support
- Audio arc: low at the hook, firmer under the terminal result, then reduced under the final hold
- Music: `happy-beats-business-moves-vol-12-by-ende-dot-app.mp3`
- Music treatment: baseline volume 0.24, short fade-in, and fade to 0.10 during the final second
- Music cue guidance: `composition/assets/music/cues/happy-beats-business-moves-vol-12-by-ende-dot-app.music-cues.json`
- Audio-reactive treatment: extracted data at `composition/assets/music/audio-data.json`; use bass and RMS only for subtle connector glow and background depth
- Audio-coupled moments:
  - 2.19 seconds: connector break
  - 8.74 seconds: terminal result
  - 13.11 seconds: ADR history
  - 19.66 seconds: final criv lockup
- SFX selection: low-risk click for the broken link, soft impact for terminal success, and restrained bell for the final lockup
- SFX analysis guidance: `.agents/skills/brag/assets/sfx/sfx-analysis.md`
- Audio files are local under `brag-output/composition/assets/`

## Hyperframes instructions

Use `hyperframes-core`, `hyperframes-animation`, `hyperframes-creative`, `hyperframes-keyframes`, and `hyperframes-cli`. This is the `brag` workflow. Do not use the general Hyperframes intent interview.

- Reuse the installed `terminal-simulator` catalog component where it fits the product story.
- Show the real terminal flow, the repository coverage values, and ADR metadata collected during this run.
- Keep all text readable.
- Use one paused GSAP timeline registered as `main`.
- Keep all motion deterministic and seek-safe.
- Mark major beat locks in comments.
- Use local audio assets only.
- Run `npx hyperframes check` before render.
