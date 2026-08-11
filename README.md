# r4at
Simple multi user chat application

## Here is the high level plan

1. Follow [tsoding tutorial](https://www.youtube.com/watch?v=BbIEuNscn_E) and implement simple multi-user chat using only standard library ✅
  - server, client, auth, rate limiting ✅
  - tls and security - ✅
2. Add separate client with UI using crossterm ✅
3. Add TUI using ratatui (but first just with crossterm) ✅  
5. Protoocol enhancements:
- introduce framing: add header to read exact size of payload instead of constant number of bytes ✅
- add types (user message, server announcement) ✅
- add something to handle this case: "if message is rate limited the client doesn't know that" ✅
6. Rewrite transport with async (tokio?) ✅

## Roadmap (next — in order)

1. **Auth experiments** 📌 — replace the current shared token (printed on the server, copy-pasted after connect) with real authentication. On the table: mutual TLS / client certificates (extends the existing rustls setup — clients authenticate themselves cryptographically), or app-level accounts (usernames + hashed passwords).
2. **Admin web dashboard** 📌 — live server info: users online, banned users, message count. Built with [topcoat](https://github.com/tokio-rs/topcoat).
3. **Persistence + message history** 📌 — store messages (e.g. sqlite via sqlx); reconnecting clients can see what they missed.
4. **Rooms / channels** 📌 — route messages to rooms instead of one global relay (a good fit for tokio broadcast channels).


## Additional things:
- add the ability to start client without ip-address ✅
- add commands to connect, disconnect and help ✅
- add status bar (to show connected/disconnected for now) ✅
- make service messages in chat colorful ✅


## To fix
  - unwraps in senders
  - i have 2 fields that both mean "client connected": status and stream. I should get rid of status. ✅
  - need to make widget for messages scrollable to show only last N messages if count is more than height of the area! ✅


## Start instructions (locally)

### First-time setup (TLS certs)
The server talks TLS, so it needs a cert + key. Generate them once — writes to `certs/` (the key is gitignored, the cert is committed and pinned into the client):
```console
$ cargo run --example gen_cert
```

### Server
```console
$ cargo run --bin server
INFO: Auth token is: <generated token>
<logs>
```
### Client
```console
$ cargo run --bin client 127.0.0.1
<paste or type token from server>
<type messages>
```
