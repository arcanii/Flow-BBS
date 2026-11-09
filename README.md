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
for a `Door`, and `call.pause`. What Flow draws comes from Flow's SDK
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
turning model where the file is a `.obj` or `.glb`. A caller in Flow sees
the pictures and models, hears the sounds and sees the effects; any other
caller sees the words in brackets, in their own colors.

### Doors

A door is any program on the server's PATH, in any language. It runs on
a pty of the call's own, the caller's window its size (pixels too), and
the caller's keys and its output pass straight through, so a door that
asks the terminal something gets the caller's answer. When the caller is
in Flow, its environment has `TERM_PROGRAM=flow-terminal`, so Flow's games
(Flow Mines, Flow Rogue, Flow Invaders) draw as they do in a Flow tab.
When it ends, the caller is back at the menu.

## Running a board

- Callers log in as the board's guest login, with no password. Any other
  login is refused, and so is a command (`ssh board ls`): a board is no
  shell.
- The host key is made the first time (Ed25519, readable by its owner
  alone) and kept: the phonebook pins it, so a board that makes a new one
  must change its entry first.
- Run it as a user of its own, on a port of its own (2222 unless set), so
  the machine's own `sshd` keeps port 22.

To list a board, add it to the phonebook
([arcanii/Flow-BBS-List](https://github.com/arcanii/Flow-BBS-List),
"Adding a board"). Its host key there is:

```
ssh-keygen -lf host_key
```

## Building

```
cargo test --workspace
cargo clippy --workspace --all-targets
```

Rust 1.95 or later.

## Licence

MIT or Apache-2.0, at your choice (`LICENSE-MIT`, `LICENSE-APACHE`).
