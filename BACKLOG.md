# Flow BBS backlog

What's planned for Flow's BBSes, roughly in the order asked for. Each item
notes what's known about doing it. Flow itself keeps its own backlog, in
its own repository, and `dial` its own, in arcanii/Flow-Phonebook.

## Boards

- **Sounds on or off**, asked for the Anie Solutions BBS on 2026-11-10
  and again on 2026-11-14: a caller turns the board's sounds off, and on
  again. A member's choice is kept with their profile (a column on
  `members`, set from Profile and security, as logging in by code alone
  is); a guest's lasts the call (a key on the welcome's menu, say).
  - **What makes a sound now:** a screen's `{sound …}` marks (the
    welcome's chime, the gallery's and the teapot's), sent by
    `screen.rs` to a caller in Flow; the menu's click when a choice is
    taken (`menu.rs`); and doors that make their own: Neko (`flow-cat`,
    through Flow's SDK's `send_effects`) and Mines (`flow-mines`).
    Dungeon makes none.
  - **Known shape:** the SDK's `Call` gets a switch for its sounds that
    `show` and the menu obey, so `flow-bbsd`'s boards can have it too;
    doors are told as they start, since each makes its own: Mines has
    `--no-effects`, which stills its liquid as well (look for a switch
    for its sounds alone), and Neko would read a variable of its own.
  - **To settle first:** whether effects on the glass (`{fx …}`) get a
    switch of their own (for a caller who'd rather the screen kept
    still), and where a guest finds the switch.

## Doors to come

- **A space trading game for many players**, moved here from Flow's
  backlog on 2026-11-11 since it needs a server: a door, blending what
  Elite and Trade Wars did (Trade Wars was itself a BBS door). Each player
  takes a cruiser through star systems drawn in 3D wireframe (a scene sent
  each frame through the Ratty protocol, as Flow-SDK's `flow-demo` does),
  with swarms of fighters to launch, fight with and lose; warps between
  systems along a map of sectors; trades at the ports, buying low and
  selling high; and sets up colonies on planets that grow, produce and
  need defending.
  - **What the board gives it:** many players share one universe, so it
    lives on a board's server: the callers are the board's members (as
    the Anie Solutions BBS keeps them), the universe is kept between
    calls (PostgreSQL, as that board's forums are), and turns or time are
    rationed per day, as the old trading doors did.
  - **What Flow gives it:** the liquid and sounds for warps, hits and
    trades (`{fx …}` and `{sound …}`, or the SDK's own calls), and the
    wireframe as models; a caller in another terminal gets text sector
    maps instead (`{not in flow}`).
  - **To settle first:** whether it's a door of `flow-bbsd`'s (a program
    a board runs, as `flow-cat` is) or part of a board in code; how often
    a scene can go down the line to many callers at once; and how players
    meet (in a sector at once, or only through what they leave behind, as
    Trade Wars' were).
