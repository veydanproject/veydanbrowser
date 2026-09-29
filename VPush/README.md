# VPush

Push server of Veydan: watches Nostr relays for the users who registered and
wakes their devices when something arrives for them.

The server never sees message contents. It sees that an encrypted event
addressed to a user appeared on a relay, and tells the user's device.

**State: 0.1.0.** Devices register, relays are watched, and a message on a
relay becomes a push through FCM. What is ahead: commands of the admin by
direct message, relays of other operators, UnifiedPush; see
[CHANGELOG.md](CHANGELOG.md).

## Layout

```
crates/vpush-proto    what the server and its clients exchange (serde only)
crates/vpush-server   the server, as a library
crates/vpush          the binary: server and admin command line in one file
deploy/               systemd unit, example config, example deploy settings
scripts/              build, deploy, rollback
spec/                 protocol for clients, operations for the one who runs it
```

This folder depends on nothing outside itself. It lives in the Veydan Space
repository for now; moving it to its own repository is moving the folder.

## Working on it

```
make test            unit tests and tests of the real binary
make check           clippy, warnings are errors
make run             the server, here, with vpush.toml next to the Makefile
make ctl ARGS="log set relay=debug --for 15m"
```

From the root of the Veydan Space repository the same targets are
`make vpush-test`, `make vpush-run` and so on.

## Putting it on a server

Once per server:

```
cp deploy/deploy.example deploy.env     # the address and the login
make bootstrap                          # user, directories, systemd unit
```

Then set `public_url` in `/opt/vpush/etc/vpush.toml` on the server.

Every release:

```
make deploy
```

It builds one static binary, sends it, lets the new binary read the live
config, saves the database, switches, and waits for the health check. A
release that does not answer is replaced by the previous one, and the command
fails.

```
make status          which release runs, is it alive
make logs            follow the log
make rollback        back to the release before this one
make server-ctl ARGS="log set relay=debug --for 15m"
```

Details: [spec/operations.md](spec/operations.md).
