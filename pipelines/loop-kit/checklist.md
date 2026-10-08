# Production checklist

Tick items as you finish them (`- [x]`). The next session resumes from the first unticked one.

## Genre
- [ ] 5+ sources read on the genre
- [ ] `music/genre.json` written · `strudel.mjs genre` passes · `mfx step done genre`

## Kit plan
- [ ] Tempo/meter/key · chart · parts on their channels
- [ ] One section per loop (the asked number and length), each with a function and an intensity · `"loops": true`, `"length_fixed": true`
- [ ] `strudel.mjs plan` passes · `music/BLUEPRINT.md` submitted

## Loops and MIDI
- [ ] Score written from the approved plan (notes only), every loop leading back into itself
- [ ] `strudel.mjs check` at 0 problems (clean loop points) · `out/music/midi/sections/` has a folder per loop · `mfx step done score`

## Sketch
- [ ] `music/instruments.json`: a clear sketch sound for every channel
- [ ] `strudel.mjs instruments` and `audition` pass · `strudel.mjs render --stems` · `mfx step done sketch`

## Final
- [ ] Kit `.mid`, sketch, cue sheet, music sheet and `midi.zip` submitted
- [ ] Every note answered in the next version (if any)
