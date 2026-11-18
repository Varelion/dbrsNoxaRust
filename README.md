# noxaDBRS

Dragon Ball: Rising Spirit's Discord bot, in Rust. One server, one SQLite file, one thread.

```
main ─┬─► discord (gateway, commands, replies) ─┐
      ├─► web (admin pages) ────────────────────┼─► services (rules) ─► store (SQLite) ─► noxadbrs.db
      │     ├─ guild port ─► discord: names, channels, pictures, mod log lines
      │     └─ relay client ─► wss://dbrs.neokai.org/relay (neoKai/relay-dbrs)
      └─► telemetry                              domain (pure rules, no I/O)

browser ─► dbrs.neokai.org (Cloudflare Worker) ═ WebSocket the bot opened ═► noxaDBRS on the Pi
```

## Commands

One command per thing; staff pick the verb from its `action` list.

| Command | Who | Does |
|---|---|---|
| `/z_moderator_admin` | staff | The admin pages, a link by DM: 30 minutes without use, 4 hours at most |
| `/z_moderator_dbrs action:mod_log` | staff | Where every change is posted. Nothing changes until it is set. |
| `/z_moderator_sp action:give\|take\|set\|grant\|history` | staff | Spirit Points, memo required, ledger kept |
| `/z_moderator_character action:add` | staff | An approved character into a player's slot |
| `/z_moderator_feat action:add\|edit\|link\|unlink\|archive\|restore\|list` | staff | Silent feats, linked to posts; archived, never deleted |
| Right-click a post › Apps › Log feat | staff | A feat from a post, its author's character prefilled |
| `/me` | everyone | Your page, a link by DM: your characters, who you play, your Spirit Points, everyone's cards |
| `/profile` | everyone | A character's card, posted in the channel |

## Web pages

`/z_moderator_admin` DMs a link to `https://dbrs.neokai.org/a/<token>`, `/me` one to `/p/<token>`. No scripts, no
cookies: the token is the login, and a player's link never opens the admin pages. A new link replaces the old one.

| Page | Manages |
|---|---|
| Players | everyone with Spirit Points or a character; give/take/set SP, the ledger, add characters |
| Characters, NPCs | every sheet: name, prefix, race, tier, title, bio, notes, picture (link or upload); McGuffins held; feats |
| McGuffins | make, edit, give, take back, retire, restore |
| Dragon Balls | search areas (categories, channels), the roll, where each ball hides, scatter |

| Player page (`/me`) | your characters as cards, who you play (one press), your Spirit Points and ledger, everyone else's cards |

An uploaded picture is posted to `PICTURES_CHANNEL` with the image attached; its link expires, so the
bot fetches a fresh one from that message when needed. The bot needs View Channel, Send Messages,
Attach Files and Read Message History there.

## What the network sees

```
browser ══ HTTPS, TLS 1.2+, HSTS; ECH when the browser does it ══► Cloudflare (dbrs.neokai.org)
                                                                      ║  the bot's socket
Pi ══ DNS over HTTPS to 1.1.1.1 ══► address + ECH keys               ║
Pi ══ TLS 1.3, Encrypted Client Hello: outer name cloudflare-ech.com ═╝
```

- No plain DNS lookup and no plain server name for the relay: Wireshark on the Pi's network sees TLS to a
  Cloudflare address, not which site. No ECH keys published, or ECH refused: the bot does not dial at all.
- Plain `http://` is sent to `https://` by the Worker and never reaches the bot; `/relay` over plain HTTP is refused.
- Visitors only ever reach Cloudflare: nothing they can see leads to the Pi.
- `RELAY_URL` must be `wss://`, `WEB_URL` `https://`, or the bot will not start.
- No plain-HTTP listener with a relay, unless `WEB_BIND` is set (and then on 127.0.0.1 only).
- Discord's own traffic is TLS too, but names discord.com: it says "a Discord bot", nothing about the site.

## Run

```
cp .env.example .env    # fill in DISCORD_TOKEN
cargo run --release
```

Discord Developer Portal › Bot: enable the **Server Members** intent (`/z_moderator_sp grant` lists a role's holders;
the admin pages list members' names, else look them up one by one).

The relay: `neoKai/relay-dbrs` (`npx wrangler deploy -c relay-dbrs/wrangler.jsonc`), its `RELAY_SECRET` the same as the bot's.

## On the Pi

Build on the Pi (`cargo build --release`), or cross-build here with
`cargo zigbuild --release --target aarch64-unknown-linux-gnu` (SQLite is compiled in, so the target needs a C compiler; zig provides one).

```
/opt/noxadbrs/noxadbrs        the binary
/opt/noxadbrs/.env         settings (chmod 600)
/opt/noxadbrs/noxadbrs.db     the data: back it up nightly, e.g. sqlite3 noxadbrs.db ".backup /backups/noxadbrs.db"
deploy/noxadbrs.service    systemd unit
```

## Logs

Everything goes to the journal (`journalctl -u noxadbrs`):

| Line | Says |
|---|---|
| `interaction … done ms=…` | every command, with who, where and how long |
| `failed reference=123456 error=…` | a failure, its causes; the user saw "ref 123456" |
| `panic … location=… backtrace=…` | a bug, where it happened |
| `audit … change line=…` | every #mod-log line, kept even if Discord refused it |
| `refused: not staff roles=[…]` | a player trying a staff command |
| `gateway closed / reconnect / resumed` | connection trouble |
| `health rss_mib=… failed=… panicked=…` | hourly; a rising rss_mib is a leak |
| `relay connected / closed / down; redialling` | the admin pages' link to dbrs.neokai.org |
| `audit … change on the admin pages` | every change made on the web, before its mod log line |

`RUST_LOG=info,noxadbrs=debug` also logs every reply's text.

## Test

```
cargo test
cargo clippy --all-targets
```
