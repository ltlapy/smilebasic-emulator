# Writing style

How this repository writes comments, documents, commit messages and text shown
to users. It is written mainly for AI agents that contribute here, and it
applies to people too.

If you already follow a writing guide, read this as a description of how we
write and match it where the two do not conflict. If you have no guide, follow
this one.

The target is the style of [CONTRIBUTING.md](../../CONTRIBUTING.md): short
declarative sentences, common technical terms, American English.

## Principles

1. **Keep the meaning.** When you edit existing text, every fact, number, name,
   condition and exception stays. Do not add facts.
2. **Leave good sentences alone.** If a sentence has none of the patterns below
   and reads clearly the first time, do not change it.
3. **Write subject, verb, object.** Put one idea in a sentence. Split a
   sentence that runs past about 30 words.
4. **Use the standard technical term.** Write `iteration`, `per`, `returns`,
   `cache`, `skips`, not a paraphrase of them.
5. **Stay plain in both directions.** Do not use literary phrasing (the table
   below). Do not swing to formal phrasing either: no `utilize`, `in order to`,
   `prior to`, `furthermore`, `thus`, `leverage`, `facilitate`.

Passive voice is fine where the actor does not matter ("The font is copied
into GRP5 at start-up"). Do not add filler such as `note that`, `simply`,
`basically` or `it is worth mentioning`. Do not use em dashes.

## Patterns to replace

These patterns are common in the older text of this repository. Replace them
when you find them.

| Pattern | Write | Before | After |
|---|---|---|---|
| `X of its own` | `its own X`, `a separate X` | fix it in a commit of its own | fix it in a separate commit |
| `as it was`, `as it is` | `unchanged` | move the code as it was | move the code unchanged |
| `anew`, `afresh` | `again` | laying out the registers anew | laying out the registers again |
| `at once` | `immediately`; `at the same time` when it means together | frees the handle at once | frees the handle immediately |
| `round` as a preposition | `around`, `repeats` | whether a FOR goes round again | whether a FOR repeats |
| `a` for a rate | `per` | about 1 ms an entry | about 1 ms per entry |
| `knows nothing of`, `says of`, `hears of` | `has no knowledge of`, `says about`, `is notified of` | This crate knows nothing of SmileBASIC. | This crate has no knowledge of SmileBASIC. |
| `goes on` | `continues` | the program goes on | the program continues |
| `makes way`, `takes away`, `leaves out` | `is stopped`, `removes`, `skips` | the oldest make way | the oldest notes are stopped |
| `go apart`, `lies` | `differ`, `is` | what lies past the end | the values past the end |
| `as X does`, `as X are` | `like X`, `the same as X does` | waits a frame, as a confirmation does | waits a frame, like a confirmation dialog |
| Two possessives in a row | `of`, or a noun used as an adjective | The editor's window's UI scale | The UI scale of the editor window |
| A possessive with no noun after it | repeat the noun | the screen's, or the editor's frame | the screen or the editor frame |
| `What`, `Where`, `How` clause as a comment | a noun phrase | What a panic said. | The panic message. |
| `as a rule`, `all the same` | `usually`, `still` | one as a rule | usually one |
| `without a word`, `for nothing` | `silently`, `needlessly` | lost its textures without a word | silently lost its textures |
| `whoever`, `whatever` | `anyone`, `any`, `anything` | For whoever changes this next | For anyone changing this |
| `need not` | `does not need to` | The caller need not check. | The caller does not need to check. |
| Contractions | the full form | don't, isn't, can't | do not, is not, cannot |
| `whose` twice in a sentence | two sentences | | |

The table gives defaults. The meaning decides. "Cleared on the next frame, as
its parent was" does not become "like its parent", because the parent was
cleared a frame earlier: write "after its parent was cleared".

More rules for these patterns:

- **`as X does`.** Use "like X" only when the two things are alike. When the
  text says that X does the same action, write "the same as X does": "Clears
  every sprite's variables, the same as starting a program does."
- **Instructions stay instructions.** When a sentence lists what the reader
  must add or do, each item stays something to do. "A new screen needs a
  handler called from the loop" does not become "The loop calls the handler".
- **Slogans.** A short "X is not Y" makes the reader work out the relation.
  State the relation: "An iteration is not always one frame."
- **The rule and its exceptions.** Give the usual case first and join the
  exceptions to it with "but".
- **Naming a term.** Define a term with "refers to" or "means", not with an
  order to the reader: `"Iteration" refers to the loop`, not `Say "iteration"
  for the loop`.
- **Other idioms.** The table is not complete. Replace any idiom with the
  literal word when the meaning is certain: "draw alike" becomes "draw the
  same way", "a build running beside it" becomes "a build running at the same
  time", "differs by itself" becomes "differs with no other change", "keep it
  so" becomes "keep it short". An idiom that is already everyday English
  stays: "run the scene again by itself".
- **Possessives.** A plain possessive is fine: "the sprite's page". Fix only
  the two cases in the table. You may restructure the sentence to do it: "the
  textures are the editor window's" becomes "the textures belong to the editor
  window".
- **`What`, `Where`, `How`.** This covers comments, table cells and list
  items. Use a noun phrase when a short and exact one exists: "What SPHITINFO
  reports." becomes "The details that SPHITINFO reports." Keep the clause when
  the noun phrase would only repeat the name of the type or field, or would
  need a word the text does not give.
- **`Whether`.** A doc comment that starts with `Whether` is the usual form
  for a boolean. Keep it.
- **A vague phrase and its technical term.** You may replace a paraphrase with
  the technical term it describes ("memory a small machine has no room to keep
  near" is the cache) only when the code or the text around it settles the
  meaning. If you had to guess, keep the original words.
- **Not the pattern.** `f.round()`, "rounds to the nearest", "its own
  rectangle" and "a frame" in "draws a frame" are correct as they are.

## Terms

- Use SmileBASIC 4's own words. Write `DIRECT mode`, not "direct mode" or "the
  prompt". A button or key is `released`, not "let go". A controller
  `vibrates`, not "shakes". Name a thing on the screen by the name the screen
  or the manual gives it.
- One pass of the window's main loop is an `iteration`, not a "trip".
- These project terms have a fixed meaning. Keep them: `place`, `entry`,
  `chain`, `step`, `slot`, `page`, `handle`, `scene`.
- Spelling is American: `color`, `behavior`, `center`, `gray`, `toward`.

## Doc comments

- A function's comment starts with a verb in the third person: "Returns the
  template number.", "Draws one row."
- A type, field or constant gets a noun phrase: "The width of the keys
  column."
- These two forms are for new comments and for comments you rewrite anyway.
  Do not rewrite a clear existing comment only to change its form.
- Comments explain why. Skip a comment that repeats the code.
- Keep the markers `// UNVERIFIED:` and `// SAFETY:` as they are spelled.

## Examples

### Rewritten

Before:

> A trip is not a frame. A trip runs no frame of the program while it is
> paused, one as a rule, several to catch up, and it may leave the drawing out.

After:

> An iteration is not always one frame. It usually runs one frame of the
> program, but it runs none while the program is paused and several when the
> program has to catch up. It may also skip drawing.

The rewrite does three things. It replaces the slogan "A trip is not a frame"
with the plain relation. It joins the usual case and its exceptions with
"but". It moves the last item into a separate sentence.

Before:

> For an AI agent or a person about to change this repository: what each crate
> is for, what to watch in it, and how to check a change. It adds to
> CONTRIBUTING.md, which has the rules for code, tests and commits; read that
> first.

After:

> This document is for an AI agent or a person about to change this
> repository. It covers what each crate is for, what to watch in it, and how to
> check a change. It adds to CONTRIBUTING.md, which has the rules for code,
> tests and commits. Read CONTRIBUTING.md first.

Before:

```rust
// The pixels that mouse positions are given in: the screen's, or the
// editor's frame.
```

After:

```rust
// The pixel space of mouse positions: the screen or the editor frame.
```

Before:

```rust
/// How many of `hints` fit, in order, across a bar `w` wide.
```

After:

```rust
/// The number of `hints` that fit, in order, on a bar `w` pixels wide.
```

The second rewrite adds "pixels". That is allowed only because the code shows
that `w` is in pixels. When the code does not settle a question, keep the
original wording.

### Left alone

These are already plain. Do not rewrite text like them.

- "Bytes of machine code."
- "SDL and platform code live in `sb-sdl` only."
- "A program is untrusted. It may fail with a SmileBASIC error."

### Rewrites to avoid

| Original | Bad rewrite | Why it is bad |
|---|---|---|
| A `GOSUB` takes 1. | A `GOSUB` typically takes about 1 slot. | It adds a hedge, and a unit the text did not give. |
| move the code as it was | relocate the code in its original form | It is formal and longer. "move the code unchanged" is enough. |
| Frames the standby thread ran this trip. | This field keeps track of the total number of frames that were run by the standby thread during the current iteration of the loop. | It is three times as long. "Frames the standby thread ran in this iteration." is enough. |

## Other languages

Korean documents end sentences with `-습니다`. Japanese documents use です/ます.
Keep one ending style through a document.
