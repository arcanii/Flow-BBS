# Flow BBS

Bulletin board systems for Flow, the terminal: boards that callers reach
with Flow's `dial` (or plain `ssh`), drawn with what Flow adds (pictures,
turning models, effects on the glass, sounds) and in plain text for
callers in other terminals.

Two crates:

- **`flow-bbs`**, the SDK (`sdk/`): a board written in Rust. A board is a
  type with a `call`, which runs once for each caller with their `Call`:
  who's calling, whether it's Flow, their window, their keys, and a way to
  write to them. `serve` puts it behind an SSH server of its own.
- **`flow-bbsd`**, the stock server (`server/`): a board run from a
  folder, for sysops who'd rather write files than Rust.
- **`anie-bbs`** (`boards/anie/`), the Anie Solutions BBS: a board in
  code with members the sysop lets in, an authenticator app at login,
  forums and a bug tracker, in PostgreSQL. It runs on tkaniedev03 in
  Docker (`boards/anie/deploy/`).

Boards are listed for `dial` in the phonebook,
[arcanii/Flow-BBS-List](https://github.com/arcanii/Flow-BBS-List).

## A board in code

```rust
use flow_bbs::{serve, Call, Config, Key};

fn main() -> std::io::Result<()> {
    serve(Config::default(), |call: &mut Call| {
        call.say(&format!("Welcome, {}. Press any key to hang up.\n", call.user()))?;
        if call.is_flow() {
            call.send(&flow_bbs::flow::fx::Effect::ripple((10, 0)).escape())?;
        }
        while let Some(key) = call.key()? {
            if key != Key::Resize {
                break;
            }
        }
        Ok(())
    })
}
```

A board in code has what `flow-bbsd` is made of: `Screen::read` and
`call.show` for screens, `call.menu` for a menu of `Item`s, `call.door`
for a `Door`, and `call.pause`; and for a board of its own drawing,
`call.clear`, `call.ask` and `call.ask_secret` for a line typed (only
printable letters kept), `call.edit` for a message written full-screen,
`printable` for showing what someone else typed, and `call.address` for
where the caller is. What Flow draws comes from Flow's SDK
([arcanii/Flow-SDK](https://github.com/arcanii/Flow-SDK)), re-exported as
`flow_bbs::flow`: its pictures, models, effects and sounds are escape
sequences, which `call.send` writes down the call.

A call knows the caller is in Flow from `TERM_PROGRAM=flow-terminal`,
which `dial` sends, else from `LC_TERMINAL=flow-terminal`, else by asking
their terminal which it is (XTVERSION), waiting a second at most. Keys
typed meanwhile are kept for the board.

## A board from a folder

```
flow-bbsd boards/example
```

```
board.toml   the name; where it listens (0.0.0.0:2222), its guest login
             (guest), its host key (host_key, made the first time); its
             menus and its doors
screens/     a text file a screen
pictures/    the PNGs and models the screens show
```

A caller starts at the menu `main` (or the one `first` names). Each item
shows a screen and comes back, goes to another menu, opens a door and
comes back, or hangs up:

```toml
name = "Example Board"

[menus.main]
screen = "welcome"
items = [
  { key = "a", label = "About this board", screen = "about" },
  { key = "g", label = "The gallery", screen = "gallery" },
  { key = "m", label = "Play Flow Mines", door = "mines" },
  { key = "q", label = "Hang up", hang_up = true },
]

[doors.mines]
command = ["flow-mines"]
```

A board with no menus shows `screens/welcome.txt` and hangs up. Every
screen, menu and door a board names is checked when `flow-bbsd` starts,
so a mistake is said then, with its file and line, not to a caller.

### Screens

A screen is lines of text, with marks in braces:

| Mark | What it does |
| --- | --- |
| `{user}`, `{board}` | The caller's login, the board's name |
| `{red}` … `{white}`, `{bright-red}` …, `{bold}`, `{dim}`, `{/}` | The sixteen colors, so the caller's theme recolors them; `{/}` goes back to plain |
| `{sound chime}` | One of Flow's named sounds, there |
| `{fx ripple}` | An effect on the glass at that line: `ripple`, `burst`, `stir`, `bolt` |
| `{{` | A brace |

A line that's only `![words](pictures/banner.png)` is a picture, or a
turning model where the file is a `.obj` or `.glb`; a glTF with an
animation of its own plays that instead of being turned. A board in code
can make its pictures and models itself and give them to `Screen::read_with`
by the names its screens use. A caller in Flow sees the pictures and
models, hears the sounds and sees the effects; any other caller sees the
words in brackets, in their own colors.

A picture is its own size in cells; `{rows 10}` after it makes it ten rows
tall whatever the size of the caller's cells, so a logo can be sent sharp
and still take the room it should. A picture or model is made smaller
where it would leave no room for the lines under it, or for a menu's
items. A line that starts `{in flow}` is shown only to a caller in Flow,
and one that starts `{not in flow}` only to any other:

```
{in flow}![Our logo](pictures/logo.png){rows 10}
{not in flow}{bold}{bright-cyan}OUR BOARD, in letters drawn for other terminals{/}
```

### Menus

A menu draws its items in a frame under its screen (`Call::menu`, or
`Call::menu_titled` with a title in the frame). The arrows move a bar over
them and Enter takes the one it's on, or an item's key takes it at once.
The frame is in the sixteen colors and the bar is the caller's theme
swapped, so both follow the caller's theme.

### Writing

`Call::edit` lets a caller write full-screen: a `Draft` gives it a title,
a line saying what to write, the text to start from, and how long it may
grow, and how the board shows it to its readers. The words wrap to the
window as they're typed. The arrows, Home and End, Page Up and Page
Down, and Ctrl with the arrows (a word at a time) move about; Ctrl+K cuts
a line and Ctrl+U pastes what was cut; Ctrl+Z undoes and Ctrl+Y redoes;
Ctrl+P (or F2) previews it as its readers will see it (lines the board
gives, or a screen, a picture and all; else the text wrapped to the
window), and any key goes back to writing; Ctrl+S saves, and Esc leaves,
asking first whether to save what changed. Only printable letters are
kept, and only the rows that changed are drawn again.

A board can draw what's written as it's written, too (`Draft::live`: a
picture, or words): the editor gives it a pane beside the text in a wide
window and under it in a narrower one, and draws it again each time the
caller stops typing a moment.

### Doors

A door is any program on the server's PATH, in any language. It runs on
a pty of the call's own, the caller's window its size (pixels too), and
the caller's keys and its output pass straight through, so a door that
asks the terminal something gets the caller's answer. When the caller is
in Flow, its environment has `TERM_PROGRAM=flow-terminal`, so Flow's games
(Flow Mines, Flow Rogue, Flow Invaders) draw as they do in a Flow tab.
Otherwise a door gets nothing of the server's environment but what a
program needs to start (`PATH`, `HOME`, the language and time zone), so a
board's settings and secrets never reach a program that callers type at.
When it ends, the caller is back at the menu.

### The cat

`flow-cat` (`doors/cat`) is a door of this repository's own: Neko, the
cat that chases the pointer, from 1989's desktops. In Flow it's Neko's own
pictures, its frames sent once and moved by their placeholder cells; it
runs to the caller's pointer facing one of eight ways, and once it's
caught up it sits, washes, scratches, yawns and falls asleep. Moving the
pointer startles it awake; where the pointer's past the window's edge it
claws at that edge; and Flow's liquid ripples when it pounces. In another
terminal it's a cat in text. Where there's no mouse, the arrows move a
mark for it to chase. The example board offers it as `c`.

Its sprites are Kenji Gotoh's for NekoDA, free to use, modify and
redistribute, carried under those terms rather than this repository's
licence (`doors/cat/sprites/NOTICE.md`).

It runs in a Flow tab of its own too: `cargo run -p flow-cat`.

## Running a board

- Callers log in as the board's guest login, with no password. Any other
  login is refused, and so is a command (`ssh board ls`): a board is no
  shell.
- The host key is made the first time (Ed25519, readable by its owner
  alone) and kept: the phonebook pins it, so a board that makes a new one
  must change its entry first.
- Run it as a user of its own, on a port of its own (2222 unless set), so
  the machine's own `sshd` keeps port 22.
- At most 64 connections at once and 4 from one address (`Config`'s
  `max_calls` and `max_calls_per_address`); past them, a connection is
  turned away before it logs in.

To list a board, add it to the phonebook
([arcanii/Flow-BBS-List](https://github.com/arcanii/Flow-BBS-List),
"Adding a board"). Its host key there is:

```
ssh-keygen -lf host_key
```

## The Anie Solutions BBS

`boards/anie` is a board in code. A caller applies from the welcome: a
handle, their name, an email address (a code is mailed to prove it), a
password, and an authenticator app set up from a QR code, with eight
recovery codes. The sysop decides each application from the board's sysop
menu. A member logs in with handle, password and the app's code; staff
moderate the forums and triage bugs; the sysop also manages members and
roles, forums, the tracker's products, the bulletin, and reads the log.
Posts, bug reports and the bulletin are written in the SDK's full-screen
editor, each previewed (Ctrl+P) as its readers will see it. In Flow the
bulletin is typeset, beside its editor as it's written too, as the
demonstrations' page of type is: Noto Serif (Noto Serif CJK for Japanese, Chinese and Korean) on
a card of cream paper, its lines starting `# ` headlines, `## ` subheads
and `- ` points; other terminals get it as text.
Guests and members alike have its game room (experimental): Mines
(`flow-mines`, from Flow-Games), each member's best time on each size kept
for a leaderboard they share; Dungeon, the mainframe original of Zork
(devshane/zork's C, public domain, built into its image without its
`!` shell escape or its debugging tool, and with its parser's words
bounded), a member's saved game kept for their next call; and Neko
(`flow-cat`, a door its image carries, with Neko's notice); and its
demonstrations: a gallery of a
picture, a turning model, an effect and a sound; Newell's teapot, as
OpenGL's GLUT drew it; a hypercube turning through four dimensions; a room
lit by three lamps of its own going round; and a page of type. The teapot,
the hypercube and the room are glTF the board makes in code, which Flow
turns and plays itself. In Flow its welcome and
main menu show Ánië's logo; other terminals get its name in letters.
Its codes follow United One's rules (arcanii/United_ONE-BackendSystem,
ADR-0013 and ADR-0024): see `boards/anie/src/crypto.rs`.

It's set up from its environment (`boards/anie/src/settings.rs`) and runs
with `docker compose` (`boards/anie/deploy/`, with the runbook for
tkaniedev03). Its tests call it over SSH against a PostgreSQL of their
own, when `ANIE_BBS_TEST_DATABASE_URL` names one:

```
ANIE_BBS_TEST_DATABASE_URL=postgres://postgres@127.0.0.1:55432/postgres cargo test -p anie-bbs
```

## Building

```
cargo test --workspace
cargo clippy --workspace --all-targets
```

Rust 1.95 or later.

## Licence

MIT or Apache-2.0, at your choice (`LICENSE-MIT`, `LICENSE-APACHE`),
but for Neko's sprites in `doors/cat/sprites/`, which are carried under
their own terms (`doors/cat/sprites/NOTICE.md`). The Anie board's image
carries Noto faces from Debian's packages, under the SIL Open Font
License, with Debian's notices of their terms
(`/usr/share/doc/anie-bbs-fonts`).
