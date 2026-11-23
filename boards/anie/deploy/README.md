# The Anie Solutions BBS on tkaniedev03

The board runs in Docker on tkaniedev03 (Ubuntu 26.04, Docker 29, Compose
5): PostgreSQL 18, `anie-bbs`, and its game room's doors (`flow-doord`,
running Neko, Mines and Dungeon) in a container of their own, the compose
project `anie-bbs`. It runs in a Docker of its own: rootless, run by the
machine's user `anie-bbs` (`15-rootless.sh`), apart from root's Docker and
the box's other projects in it, so nothing of the BBS runs as root, and a
container broken out of is that user, who has the BBS's files and nothing
else of the machine's. Every change to the box is a script in `box/`,
signed and run through the Ánië console (Anie_Maint), so each is Bryan's to
approve and is kept in the central's audit. The scripts run as root and do
what's the BBS's as `anie-bbs` (`runuser`, its Docker at
`/run/user/<uid>/docker.sock`). The console signs scripts from its own
roots, so the scripts are copied to `G:/Anie_Maint/out/anie-bbs/` (Unix
line endings) to be signed from there.

| Where | What |
| --- | --- |
| user `anie-bbs`, home `/srv/anie-bbs` | The BBS's own: a system user with no login, subordinate ids 300000-365535, its user manager kept running (linger) with the cgroup controllers its limits need, and its rootless Docker (slirp4netns, for its network and its published ports) |
| `/srv/anie-bbs/flow-bbs` | The clone of arcanii/Flow-BBS, `anie-bbs`'s, at the commit on `main` that `02-clone.sh` names |
| `/srv/anie-bbs/flow-games` | Flow-Games' tree at the pinned commit (no `.git`, no key), exported by `11-games-clone.sh`: the doors' image's second context |
| `/srv/anie-bbs/secrets/bbs.env` | `ANIE_BBS_DB_PASSWORD`, `ANIE_BBS_SECRET_KEY`: made on the box, never printed; `anie-bbs`'s, mode 600 |
| `/srv/anie-bbs/secrets/smtp.env` | Gmail's relay and its app password: written by Bryan, by hand |
| `/root/anie-bbs/bbs.env`, `smtp.env` | Root's copies from before the BBS had a user of its own, which `03-secrets.sh` copied from |
| `/root/anie-bbs/flow-games-key` | The deploy key that reads arcanii/Flow-Games (private), for `flow-mines`: root's, never the BBS's user's, never leaves the box; read-only on GitHub |
| `/root/anie-bbs/flow-games` | The clone of Flow-Games, root's, at the commit on `master` that `11-games-clone.sh` names |
| `/root/anie-bbs/backups/` | Nightly `pg_dump`s, fourteen days: root's, out of the BBS's user's reach |
| volumes `anie-bbs_db`, `anie-bbs_home` | In `anie-bbs`'s Docker: the database; the board's SSH host key |
| volumes `anie-bbs_doors-kept`, `anie-bbs_doors-socket` | The doors' own: members' saved games (Dungeon's under `dungeon/<member>`), the doors' user's (10002) alone; and the socket the board reaches the doors at, which only the two containers mount |
| `0.0.0.0:2222` | The board, published by `anie-bbs`'s Docker (slirp4netns, which hands the board each caller's own address); ufw decides who reaches it. The database is reached by the board alone, on their network |
| `arcanii.dynu.net:50768` | The board from the internet: the router forwards it to 192.168.50.210:2222 |

**Keep a copy of `bbs.env` offline.** `ANIE_BBS_SECRET_KEY` seals every
member's authenticator seed and keys their codes' hashes: a board without
it has members who can't log in (the sysop can reset each one's
authenticator). It's kept apart from the backups on purpose: a dump alone
opens no seed.

## Bringing it up

| Step | Script | What it does |
| --- | --- | --- |
| 1 | `01-inspect.sh` | Read-only: what's on the box |
| — | apt | `uidmap` (newuidmap, newgidmap), for rootless Docker |
| 15 | `15-rootless.sh` | Once: the user `anie-bbs`, its ids, its manager and its rootless Docker |
| 16 | `16-address-check.sh` | That a port its Docker publishes hands the container the caller's own address |
| 2 | `02-clone.sh` | The clone, at the pinned commit |
| 3 | `03-secrets.sh` | `bbs.env` made (never over one that's there, and root's copied where it's there); `smtp.env`'s template |
| 4 | `04-build.sh` | The board's image built (with the bulletin's Noto faces, about 55 MB, from Debian's packages), and its doors' (with Dungeon from devshane/zork, and Mines); PostgreSQL's pulled |
| — | Bryan, by hand | The Gmail app password into `smtp.env` (below) |
| 5 | `05-check-mail.sh` | Read-only: `smtp.env` in shape, and Gmail lets the board log in |
| 6 | `06-start.sh` | All up; waits for SSH on 2222; prints the host key's fingerprint |
| 7 | `07-firewall.sh` | ufw: 2222/tcp from anywhere |
| — | Bryan, at the router | Forward TCP 50768 to 192.168.50.210:2222 |
| — | Bryan, on the board | Apply as anyone does: handle, email, password, authenticator |
| 8 | `08-backups.sh` | The nightly backup timer, and one backup now |
| 9 | `09-sysop.sh` | Makes Bryan's handle the sysop |
| 10 | `10-games-key.sh` | Once: the deploy key for Flow-Games made (never over one that's there), its public half printed |
| — | On GitHub | That public half added to arcanii/Flow-Games as a deploy key, read-only |
| 11 | `11-games-clone.sh` | Flow-Games' clone, at the pinned commit, and its tree for the build; before each build from then on |

Done once, in the board's past, and not for running again: `12-move-saves.sh`
and `13-clear-old-saves.sh` (members' saves into the doors' volume, then the
old copies gone, under root's Docker); `17-move-in.sh` (the BBS from root's
Docker into `anie-bbs`'s, between steps 4 and 6: root's stopped for good,
its database dumped and restored, its host key and kept games copied, the
rows counted on both sides), with `18-move-back.sh` to go back if it failed.

### The doors

Neko, Mines and Dungeon run in `anie-bbs-doors-1`, by `flow-doord`, as the
user `doors` (10002), with no network (`network_mode: none`), a read-only
root, no capabilities, and none of the board's secrets, its host key or
its volume: Dungeon is 1980s C that reads what callers type, so a slip in
it has the doors' folders and nothing more. The board connects to them at
`/run/anie-doors/doors.sock`, in a volume only the two containers mount,
and the doors' server puts each game in a folder of its own: a member's
Dungeon in one kept for them, anything else in one of the call's, gone
when the game ends.

### The Gmail app password

On the Google account the board mails from (2-Step Verification on):
Security, App passwords, a new one named "Anie BBS". Then on tkaniedev03:

```
sudo nano /srv/anie-bbs/secrets/smtp.env
```

and fill in `SMTP_USERNAME` and `SMTP_FROM` (both the Gmail address) and
`SMTP_PASSWORD` (the sixteen letters, spaces or not). Save, and step 5
says whether it's right without printing it.

## Calling it

```
ssh -p 50768 guest@arcanii.dynu.net
```

(`ssh -p 2222 guest@192.168.50.210` on the LAN), or from Flow's phonebook
once it's listed (arcanii/Flow-BBS-List, with the host key `06-start.sh`
printed: `SHA256:87Vz5v/WU2LpFblsOuncrVeEtDdHTpsXCOU4HsQw+8k`).

## Upkeep

`status.sh` is read-only: the BBS's own Docker and its containers (and
that root's runs none of them), members by status, open bugs, logins and
failures in the last day, the last backup, disk, and the board's recent
log. Approve it once with a repeat window and it can be run any number of
times in it.

A new version: `02-clone.sh` at the new commit, `11-games-clone.sh` at
Flow-Games' (the same one, unless Mines is to change), `04-build.sh`,
`06-start.sh` (compose recreates the board's container and the doors'; the
database, host key and saved games stay).
The schema migrates itself when the board starts.

A restore, as `anie-bbs` with its Docker: stop the board
(`docker compose ... stop bbs`), then
`docker exec -i anie-bbs-db-1 pg_restore -U anie_bbs -d anie_bbs --clean --if-exists < the.dump`,
then start it.
