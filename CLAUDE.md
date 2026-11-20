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
ANIE_BBS_TEST_DATABASE_URL=postgres://postgres@127.0.0.1:55432/postgres cargo test -p anie-bbs   # the Anie board's calls, on a PostgreSQL
ANIE_BBS_TEST_FONTS=<a folder of the image's Noto faces> cargo test -p anie-bbs   # the bulletin typeset too (ANIE_BBS_TEST_CARD=<file> keeps a card to look at)
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
  extras; lettre (the Anie board's mail) the same, `rustls-tls`.
- The toolchain here defaults to 1.92; the workspace needs 1.95, so
  `cargo +stable`.

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
- `sdk/src/key.rs`: keys from the caller's bytes (the arrows, Home, End,
  Delete, Page Up and Down as xterm, the Linux console and VT220s send
  them), a cut-off ESC waiting 50 ms for the rest of a sequence.
- `sdk/src/screen.rs`: `Screen`, its marks (`{user}`, the sixteen colors,
  `{sound …}`, `{fx …}`), its picture and model lines (`{rows N}` after
  one; a file of the board's folder, or one the board made, by name, for
  `read_with`), lines for Flow alone or for others (`{in flow}`,
  `{not in flow}`), and
  `Call::show`, which clears the screen, forgets the last screen's
  pictures and models (each call's ids its own), and places pictures by
  placeholder cells and models over the rows it makes room for, turned
  by Flow (`animate=1`) unless a glTF has an animation of its own
  (`moves_itself`). A caller not in Flow gets the words in brackets.
- `sdk/src/menu.rs`: `Item`, `Call::menu` and `menu_titled` (the items in
  a frame, a bar the arrows move and Enter takes, or a key at once;
  Ctrl+C and Ctrl+D hang up; a resize draws it again; `MENU_KEYS` says
  the keys under it) and `Call::pause`. `Call::show_leaving` leaves the
  frame room under a screen's pictures.
- `sdk/src/door.rs`: `Door` and `Call::door`, on portable-pty: the
  caller's keys to the pty, its output to the caller from a thread of its
  own, resizes passed on, Flow's variables set; the server's own
  environment cleared but for `KEPT` (`PATH`, `HOME`, the locale), so a
  board's secrets never reach a door. `command` is how a door is run,
  for the door server too.
- `sdk/src/remote.rs`: doors served elsewhere, the board's side:
  `RemoteDoor` (a socket, a door's name, a folder kept by name or the
  call's, text files to put, look at and take), `Call::open_door` (an
  `OpenDoor`, its files' times) and `Call::run_door` (an `Ended`, its code
  and the files given back; a hang-up still waits for them); and the
  frames both sides write (a kind, a length, the bytes: hello, ready and
  refused, end in JSON). Unix sockets only; elsewhere the door isn't there.
- `sdk/src/door_server.rs` (Unix): `Doors` and `serve_doors`, the server's
  side: a connection a door, its hello checked (names, paths kept in the
  folder, no links followed), its folder made 0700 (kept under
  `<kept>/<door>/<name>`, or the call's in the temporary folder, removed
  before the end is said), `HOME` and `XDG_DATA_HOME` the folder, at most
  `most` at once. `doord/`: `flow-doord`, its command line.
- `server/src/main.rs`: `flow-bbsd`, reading `board.toml` (menus, items,
  doors; every name checked at the start) and running a call through its
  menus.
- `doors/cat`: `flow-cat`, Neko, the cat that chases the pointer, a door
  of the example board's. `sprites/` is 32 of Neko's 32 × 32 frames
  (Kenji Gotoh's NekoDA sprites, free to use, modify and redistribute, as
  `sprites/NOTICE.md` says: keep that notice with them, and in a release's
  notices), carried in the program and drawn bigger pixel by pixel by
  `sprites.rs`; `cat.rs` what it does each tick (pure, tested); `main.rs` the
  terminal: Flow's SDK's `Query` for pictures and a cell's size, the frames
  sent once (as strips with bare cells where `Caps::bare`, on Windows), the
  mouse through crossterm, effects and sounds through `send_effects`.
- `sdk/src/input.rs`: `Call::ask` and `Call::ask_secret` (a typed line,
  printable letters only), and `printable` for what another caller wrote.
- `sdk/src/edit.rs`: `Call::edit`, a full-screen editor for a `Draft` (a
  title, a hint, the text to start from, its limits): words wrapped to the
  window, the arrows, Home/End, the page keys and Ctrl+arrows by words,
  Ctrl+K cutting lines and Ctrl+U pasting them (as nano), Ctrl+Z and
  Ctrl+Y, Ctrl+P (or F2) previewing, Ctrl+S saving and Esc leaving
  (asking where it changed); `EDIT_KEYS` says the keys along the bottom.
  A preview is the board's (`Draft::preview`, a `Previewer` giving a
  `Preview`: lines it scrolls, or a `Screen` and lines under it), else
  the text wrapped to the window; any key but the scrolling ones goes
  back. Where the board draws it as it's written (`Draft::live`, a
  `LiveView` giving a `Live`: a PNG and its rows, or words), a pane
  shows it beside the text (half a window of 96 cells or more, to 82) or
  under it, drawn again once keys stop for 200 ms (`Call::key_within`),
  the new picture in before the old one goes. `Text` is the text and the
  cursor, pure; `Editor` lays the window out and draws only the rows that
  changed, and nothing while keys wait.
  `server.rs` also turns away connections past `max_calls` (and per
  address) before they log in, and gives each `Call` its `address`.
- `boards/anie`: `anie-bbs`, the Anie Solutions BBS, a board in code on
  PostgreSQL (the sync `postgres` crate, a connection a call):
  `account.rs` (applying, emailed codes, the authenticator, logins,
  recovery, profile; logging in by the code alone, a member's choice,
  `gets_in`, with failed logins capped a day as well as in a row: past
  `DAY_CAP` a handle is `held` and takes a mailed code too, until its
  member proves their address), `forums.rs`, `tracker.rs`, `sysop.rs`, `games.rs`
  (the game room: its doors `neko`, `mines` and `dungeon`, all the door
  server's at `ANIE_BBS_DOORS`, in a container of their own as
  `deploy/compose.yaml` has it: Mines' best times put in its folder and
  `flow-mines/best.json` taken back for the leaderboard, `mines_best`;
  Dungeon in a folder kept by the member's number so its SAVE keeps (the
  game starts afresh all the same, so the board asks when `dsave.dat`
  last changed and says it, `saved_on`), a guest's in one of the call's;
  `deploy/Dockerfile` builds it from devshane/zork at a
  pinned commit with `deploy/dungeon.patch`, which takes out its `!`
  shell escape, bounds its parser's words, puts back the original's
  "patch you?" after a death (devshane's copy always quit there) and
  ends its yes-or-no question at the end of input, and without GDT),
  `demo.rs` (the demonstrations' menu; the pictures,
  Ánië's logo among them, in `screens/pictures`, and `type.png` set
  from `type.svg` by Inkscape), `models/` (the GLBs the demonstrations
  show, made in code and named `made/…` in their screens: `glb.rs`, a
  writer of smooth surfaces, materials, point lights and a looped
  animation; `teapot.rs`, GLUT's patches of Newell's teapot;
  `hypercube.rs`; `lamps.rs`), `typeset.rs` (the bulletin typeset for
  Flow with cosmic-text on a card like `type.svg`'s page, its lines' marks
  `# `, `## `, `- `; the faces from `ANIE_BBS_FONTS`, which the image's
  `fonts` stage copies from Debian's Noto packages with their notices; a
  card a whole number of rows tall at twice the cells' pixels; text for
  other terminals, or where there are no faces or room), `crypto.rs`
  (United One's rules for codes), `mail.rs` (Gmail by SMTP), `db.rs`
  (migrations in `migrations/`, errors logged by SQLSTATE only). Posts,
  bug reports, comments and the bulletin are written in the SDK's
  editor (`ui::editor`), each previewed as its page shows it (a post or a
  comment under its author, `ui::as_posted`; the bulletin by
  `session::bulletin_page`, typeset where it can be), and the bulletin
  typeset beside it as it's written, in Flow (`session::bulletin_card`,
  the card a member with the sysop's window would see). What a
  caller is told never says which part of a login was wrong, or whether
  an address is a member's. `deploy/`: its Dockerfile (two images, the
  board's `bbs` and the doors' `doors`, user 10002), compose file (the
  doors with no network, sharing only the socket's volume with the
  board), and the signed scripts that run it on tkaniedev03
  (`deploy/README.md`; `12-move-saves.sh` copied members' saves from the
  board's volume to the doors' once).
- `boards/example`: the example board, its banner drawn in code once
  (`pictures/banner.png`) and a crystal (`pictures/crystal.obj`); its doors
  are `flow-cat` and `flow-mines`, which must be on the server's PATH.

## Never without the user's word

Pushing, publishing a crate, and running a board where others can reach
it.
