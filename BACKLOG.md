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

- **An emulators section**, asked on 2026-11-20: a section of a board
  (beside the Anie Solutions BBS's game room, say) where a caller runs a
  server in QEMU from a disk image, or installs one from install media
  the board keeps. Servers only, with no screen: the caller has the
  machine's serial console, and nothing else.
  - **Known shape:** QEMU is a door like the others, run on the call's
    pty, the guest's first serial port on its standard input and output
    (`-display none -nodefaults -serial stdio`, or `-chardev
    stdio,signal=off` so that Ctrl+C reaches the guest and doesn't end
    QEMU). A member's machine is a qcow2 disk of their own, kept between
    calls as Dungeon's saves are, with a quota. It boots from the
    install media the first time and from its own disk after that. The
    media are read-only ISOs the sysop chooses, fetched at pinned
    addresses with their checksums, as the image fetches devshane/zork.
    At hundreds of megabytes to several gigabytes each, they would
    likely live on a volume of their own rather than in the board's
    image.
  - **Install media on a serial line:** an installer that talks only to
    a screen shows nothing here, and many boot menus are drawn on the
    screen alone. Some media boot on a serial console as they are, to be
    checked one by one. For others, the board can boot the installer's
    own kernel and initrd straight from the ISO (`-kernel`, `-initrd`,
    `-append "console=ttyS0,115200"`), as Debian's `install.amd/`
    allows. Cloud images (Debian's and Ubuntu's, already on a serial
    console, set up by cloud-init) need no install at all, if a ready
    machine is wanted as well.
  - **What stands in the way:**
    - QEMU's monitor must never reach a caller. `-nographic` puts it
      on the console behind Ctrl+A, and from it a caller could change
      the machine, attach the host's files, and run a shell command on
      the host (`migrate "exec:…"`). So: `-monitor none`, and QMP, if
      the board wants it for shutting a machine down, only on a socket
      the board alone holds. Add `-sandbox on` with `spawn=deny` and
      `elevateprivileges=deny`.
    - A caller is root on a whole machine, by design, so a bug in QEMU
      would run as whoever runs it. It belongs apart from the board, as
      the game room's doors are (`flow-doord`, in a container of its own
      with no board secrets and no network): a door of that server's, or
      a server of its own with `/dev/kvm` and nothing else.
    - Without `/dev/kvm`, QEMU emulates every instruction, many times
      slower, and an install could take an hour. KVM needs the device
      passed into the container, and tkaniedev03 to have it, which needs
      nested virtualization if it is itself a VM.
    - The network: none by default (`-nic none`). An installer that
      fetches its packages needs a mirror, and QEMU's user-mode network
      would give the guest the internet as the box's own address, so
      anything a caller does there (mail, scans) is traced to the board.
    - A serial line carries no window size, so the guest thinks it has
      80 × 24 until told (`stty rows … cols …`), and resizes don't reach
      it.
  - **To settle first:**
    - Who gets a machine: members, or a role the sysop grants, since
      each machine takes the box's memory, processors and disk.
    - How many machines run at once, and each one's memory, processors,
      disk and time a call.
    - Which media to offer.
    - The network: none, only a mirror or package proxy of the board's
      (`restrict=on` with `guestfwd`), or open.
    - What hanging up does: shut the machine down (an ACPI power-off
      over QMP, then killed after a while), or leave it running for the
      member to come back to, as `screen` would.
    - How a caller gets back to the board without shutting the machine
      down: the SDK's `Door` passes every key through, so it needs an
      escape key of its own that it watches for (Ctrl+], as telnet's).

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
