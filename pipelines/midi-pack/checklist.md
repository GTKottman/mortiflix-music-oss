# Production checklist

Tick items as you finish them (`- [x]`). The next session resumes from the first unticked one.

## Genre
- [ ] 5+ sources read on the genre
- [ ] `music/genre.json` written · `strudel.mjs genre` passes · `mfx step done genre`

## Blueprint
- [ ] The owner's MIDI read (if any in `input/`): tempo, key, length, what it plays
- [ ] Dramatic reading · genre signature · hook · tempo/meter/key · chart · every asked-for part on its own channel · sections
- [ ] `strudel.mjs plan` passes · `music/BLUEPRINT.md` submitted

## Score and MIDI
- [ ] Score written from the approved blueprint (notes only; the owner's MIDI kept)
- [ ] `strudel.mjs check` at 0 problems · `out/music/midi/` has the arrangement, every channel, `sections/`, `CUE-SHEET.md` · `mfx step done score`

## Sketch
- [ ] `music/instruments.json`: a clear sketch sound for every channel
- [ ] `strudel.mjs instruments` and `audition` pass · `strudel.mjs render --stems` · `mfx step done sketch`

## Final
- [ ] Arrangement `.mid`, sketch, cue sheet, music sheet and `midi.zip` submitted
- [ ] Every note answered in the next version (if any)
