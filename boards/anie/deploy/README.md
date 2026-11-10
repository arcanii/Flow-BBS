# The Anie Solutions BBS on tkaniedev03

The board runs in Docker on tkaniedev03 (Ubuntu 26.04, Docker 29, Compose
5): PostgreSQL 18 and `anie-bbs`, the compose project `anie-bbs`. Every
change to the box is a script in `box/`, signed and run through the Ánië
console (Anie_Maint), so each is Bryan's to approve and is kept in the
central's audit. The console signs scripts from its own roots, so the
scripts are copied to `G:/Anie_Maint/out/anie-bbs/` (Unix line endings) to
be signed from there.

| Where | What |
| --- | --- |
| `/home/bryan/flow-bbs` | The clone of arcanii/Flow-BBS, bryan's, at the commit on `main` that `02-clone.sh` names |
| `/root/anie-bbs/bbs.env` | `ANIE_BBS_DB_PASSWORD`, `ANIE_BBS_SECRET_KEY`: made on the box, never printed |
| `/root/anie-bbs/smtp.env` | Gmail's relay and its app password: written by Bryan, by hand |
| `/root/anie-bbs/flow-games-key` | The deploy key that reads arcanii/Flow-Games (private), for `flow-mines`: made on the box, never leaves it; read-only on GitHub |
| `/root/anie-bbs/flow-games` | The clone of Flow-Games, root's, at the commit on `master` that `11-games-clone.sh` names: the image's second context |
| `/root/anie-bbs/backups/` | Nightly `pg_dump`s, fourteen days |
| volumes `anie-bbs_db`, `anie-bbs_home` | The database; the board's SSH host key, and members' saved games (`saves/`, Dungeon's under `saves/dungeon/<member>`) |
| `127.0.0.1:5440` | PostgreSQL, the box's loopback only |
| `0.0.0.0:2222` | The board (the host's network, so ufw decides who reaches it) |
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
| 2 | `02-clone.sh` | The clone, at the pinned commit |
| 3 | `03-secrets.sh` | `bbs.env` made (never over one that's there); `smtp.env`'s template |
| 4 | `04-build.sh` | The board's image built (with Dungeon from devshane/zork, and the bulletin's Noto faces, about 55 MB, from Debian's packages), PostgreSQL's pulled |
| — | Bryan, by hand | The Gmail app password into `smtp.env` (below) |
| 5 | `05-check-mail.sh` | Read-only: `smtp.env` in shape, and Gmail lets the board log in |
| 6 | `06-start.sh` | Both up; waits for SSH on 2222; prints the host key's fingerprint |
| 7 | `07-firewall.sh` | ufw: 2222/tcp from anywhere |
| — | Bryan, at the router | Forward TCP 50768 to 192.168.50.210:2222 |
| — | Bryan, on the board | Apply as anyone does: handle, email, password, authenticator |
| 8 | `08-backups.sh` | The nightly backup timer, and one backup now |
| 9 | `09-sysop.sh` | Makes Bryan's handle the sysop |
| 10 | `10-games-key.sh` | Once: the deploy key for Flow-Games made (never over one that's there), its public half printed |
| — | On GitHub | That public half added to arcanii/Flow-Games as a deploy key, read-only |
| 11 | `11-games-clone.sh` | Flow-Games' clone, at the pinned commit; before each build from then on |

### The Gmail app password

On the Google account the board mails from (2-Step Verification on):
Security, App passwords, a new one named "Anie BBS". Then on tkaniedev03:

```
sudo nano /root/anie-bbs/smtp.env
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

`status.sh` is read-only: the containers, members by status, open bugs,
logins and failures in the last day, the last backup, disk, and the board's
recent log. Approve it once with a repeat window and it can be run any
number of times in it.

A new version: `02-clone.sh` at the new commit, `11-games-clone.sh` at
Flow-Games' (the same one, unless Mines is to change), `04-build.sh`,
`06-start.sh` (compose recreates the board's container; the database and
host key stay).
The schema migrates itself when the board starts.

A restore: stop the board (`docker compose ... stop bbs`), then
`docker exec -i anie-bbs-db-1 pg_restore -U anie_bbs -d anie_bbs --clean --if-exists < the.dump`,
then start it.
