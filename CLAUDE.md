# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Flow-BBS (arcanii/Flow-BBS, public) is the server side of Flow's BBSes:
`flow-bbs` (`sdk/`), the SDK a board is written with, and `flow-bbsd`
(`server/`), the stock server that runs a board from a folder. Callers
reach a board with `dial`, from Flow's app `flow-phonebook`
(arcanii/Flow-Phonebook), which finds it in the phonebook
(arcanii/Flow-BBS-List). The design is the doc "Flow Phonebook — design":
"The board's side", "The board server and its SDK", and the order of
work's step 2, which this repository is.

## Commands

```bash
cargo test --workspace                  # unit tests, then calls over real SSH (sdk/tests/calls.rs)
cargo clippy --workspace --all-targets  # no warnings
cargo run -p flow-bbsd -- boards/example   # the example board on 0.0.0.0:2222
ssh -p 2222 guest@localhost             # call it
```

- Don't run `cargo fmt`: the code isn't rustfmt-formatted (lines often
  pass 100 columns), as in Flow's repositories. Match the style by hand.
- Comments and docs are plain English that say why; each module starts
  with a `//!` summary. Tests are named as sentences.
- `flow-sdk` comes by the same tag of arcanii/Flow-SDK as Flow-Utils takes
  it (`v0.1.3`, the root `Cargo.toml`). Its escape builders (pictures,
  models, effects, sounds) are used as they are; its terminal side
  (`detect`, `term`, `input`) is for a program at a terminal of its own,
  so `Call` does that part for a call.
- russh with `ring`, not aws-lc-rs, so it builds without a C toolchain's
  extras.

## How it's laid out

- `sdk/src/server.rs`: russh's server half. Guest logins only; one session
  channel a connection; its pty (term, cells, pixels), its environment,
  then a shell starts the board's `call` on a thread of its own. Keys go
  to the call through a std channel; what it writes comes back through a
  tokio one to a task that sends it down the channel, and its exit status
  ends the call. Commands are refused. `host_key` makes the key once.
- `sdk/src/call.rs`: `Call`, and finding out who's calling (Flow's
  variables, else XTVERSION with DA1 after it, keys typed meanwhile kept).
  `say` turns each newline into CR LF: the call has no pty of its own.
- `sdk/src/key.rs`: keys from the caller's bytes, a cut-off ESC waiting
  50 ms for the rest of a sequence.
- `sdk/src/screen.rs`: `Screen`, its marks (`{user}`, the sixteen colors,
  `{sound …}`, `{fx …}`) and its picture and model lines, and
  `Call::show`, which clears the screen, forgets the last screen's
  pictures and models (each call's ids its own), and places pictures by
  placeholder cells and models over the rows it makes room for. A caller
  not in Flow gets the words in brackets.
- `sdk/src/menu.rs`: `Item`, `Call::menu` (Ctrl+C and Ctrl+D hang up; a
  resize draws it again) and `Call::pause`.
- `sdk/src/door.rs`: `Door` and `Call::door`, on portable-pty: the
  caller's keys to the pty, its output to the caller from a thread of its
  own, resizes passed on, Flow's variables set or taken out.
- `server/src/main.rs`: `flow-bbsd`, reading `board.toml` (menus, items,
  doors; every name checked at the start) and running a call through its
  menus.
- `boards/example`: the example board, its banner drawn in code once
  (`pictures/banner.png`) and a crystal (`pictures/crystal.obj`); its door
  is `flow-mines`, which must be on the server's PATH.

## Never without the user's word

Pushing, publishing a crate, and running a board where others can reach
it.
