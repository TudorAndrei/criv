# Brag plan: criv

## What is this app?

criv is a local Rust CLI that keeps repository documentation and ADR history connected to source code and policy rules.

## The nine planning answers

1. criv keeps repository documentation connected to the code it describes.
2. The strongest claim is concrete: criv validates documentation, source references, and architectural decisions before a change lands.
3. The visual hook is a broken line between a document and its source file. criv finds that break.
4. Show a real terminal flow, current coverage, and a real ADR supersession chain.
5. The shortest complete story is 25 seconds.
6. Tone preset: cinematic. Creative direction: restrained systems trailer for a local developer tool.
7. Use a steady, clean music bed with three restrained interaction and reveal sounds.
8. Share caption: "Documentation drifts. criv keeps ADR history explicit, validates supersession, and checks the links between decisions and code before a change lands."
9. User flow: run `criv check`, inspect governed source coverage, and follow the active ADR chain.

## The angle

Documentation drift is shown as a physical break between a note and its source. criv reports the result in a real terminal, then shows how accepted decisions form an append-only supersession history.

## Hook, first 2.5 seconds

The words "Documentation drifts." lock into the left side of the frame. A note card and a Rust source card move apart on the right. Their connector snaps.

## Key moments

- A terminal types `criv check` and returns `criv check: ok`.
- The terminal runs `criv query coverage --format json` and returns the current proof point: 355 source files, 355 governed files, 0 ungoverned files.
- A real chain shows ADR-0038 through ADR-0059. Each new decision supersedes the earlier one.
- ADR-0012 states the rule: accepted ADRs are immutable.

## Outro

The criv wordmark lands with the site line: "Keep repository documentation connected to the code it describes."

## User flow worth showing

Run `criv check`, query coverage, and inspect the effective ADR history.

## Tone

- Preset: cinematic
- Creative direction: restrained systems trailer for a local developer tool
- Interpretation: Use large monospace type, hard structural cuts, careful holds, and controlled motion. The claims stay factual.

## Format: landscape, 1920x1080

## Duration: 25 seconds

## Visual identity

- Background: `#101010`
- Accent: `#d8d8d8`
- Text: `#d8d8d8`
- Display font: `ui-monospace, SFMono-Regular, Menlo, Consolas, monospace`
- Body font: `ui-monospace, SFMono-Regular, Menlo, Consolas, monospace`
- Strongest visual element: the black and white site identity combined with terminal output and a vertical ADR history

## Share copy, draft

Documentation drifts. criv keeps ADR history explicit, validates supersession, and checks the links between decisions and code before a change lands.

## Audio direction

- Role: cinematic support
- Music: `spira-breakbeat-loop-gm-120bpm-by-ohpalmusic.mp3`
- Music treatment: 0.10 volume, short fade-in, and a fade under the final lockup
- Music cue guidance: analyzed preset at `composition/assets/music/cues/spira-breakbeat-loop-gm-120bpm-by-ohpalmusic.music-cues.json`, 120.19 BPM. Target 8.41 seconds for the terminal result, 13.40 seconds for the ADR history, and 19.90 seconds for the final brand reveal.
- Audio-reactive treatment: subtle connector and background response from extracted bass and RMS data
- SFX posture: sparse and motion-matched
- Audio-coupled moments: connector snap at 2.40 seconds, terminal result at 8.41 seconds, ADR history at 13.40 seconds, final criv lockup at 19.90 seconds
- Restraint rule: no sound for every text line, no loud impacts, and no equalizer visuals

## Storyboard

### Scene 1: Drift, 4.4 seconds

The site headline becomes the problem statement. A document card and a Rust source card separate. Their line breaks. The short headline holds for more than two seconds.

Sequential or interaction: the two cards move apart, then the connector breaks.

Audio intent: low music bed and one dry click when the link breaks.

Audio-coupled idea: connector break near the 2.40 second beat.

Transition mood: hard structural wipe into Scene 2.

### Scene 2: Check, 4.9 seconds

A terminal based on the real CLI flow types `criv check`. It returns `criv check: ok`. The words "before the change lands" appear as a supporting claim.

Sequential or interaction: command types, the cursor waits, and the result appears.

Audio intent: music gains a little presence. A soft impact supports the result.

Audio-coupled idea: result lands on the 8.41 second strong cue.

Transition mood: clean push into Scene 3.

### Scene 3: Proof, 4.1 seconds

The terminal queries coverage. The real current values appear: 355 source files, 355 governed files, and 0 ungoverned files.

Sequential or interaction: coverage rows arrive one by one with enough settled time to read.

Audio intent: this is the evidence peak. Use the bed and a restrained terminal click.

Audio-coupled idea: the proof settles before the 13.40 second change.

Transition mood: focused push into Scene 4.

### Scene 4: ADR history, 6.5 seconds

A vertical decision timeline uses the real ADR-0038 → 0039 → 0040 → 0041 → 0056 → 0059 chain. Old decisions stay visible as superseded. ADR-0059 lands as the accepted decision. ADR-0012 names the immutable-history rule.

Sequential or interaction: the line grows, then each ADR and supersession label appears in order.

Audio intent: the chain has a steady rise. The current decision gets one restrained emphasis.

Audio-coupled idea: the timeline begins on the 13.40 second strong cue.

Transition mood: the history moves left into the final lockup.

### Scene 5: Connected, 5.1 seconds

The criv mark and name land. The site line holds: "Keep repository documentation connected to the code it describes." A small footer reads "local Rust CLI / no server / no network".

Sequential or interaction: logo lands first, tagline follows, footer settles last.

Audio intent: one deep but quiet final accent. Music fades through the hold.

Audio-coupled idea: final lockup lands at 19.90 seconds.

Transition mood: hold on the final frame.

**Music mood for this video:** cinematic

**Audio summary:** A steady clean bed supports one break, one proof point, and one final lockup.
