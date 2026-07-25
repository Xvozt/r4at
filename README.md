# r4at
Simple multi user chat application

## Here is the high level plan

1. Follow [tsoding tutorial](https://www.youtube.com/watch?v=BbIEuNscn_E) and implement simple multi-user chat using only standard library ✅
  - server, client, auth, rate limiting ✅
  - tls and security - 📌
2. Add separate client with UI using crossterm ✅
3. Add TUI using ratatui (but first just with crossterm) ✅  
5. Protoocol enhancements:
- introduce framing: add header to read exact size of payload instead of constant number of bytes ✅
- add types (user message, server announcement) ✅
- add something to handle this case: "if message is rate limited the client doesn't know that" ✅
6. Rewrite transport with async (tokio?) 📌
8. Create web admin dashboard with some info: users online, banned users, message count (topcoat?)
9. ... "no tengo ni una idea" what i will do after


## Additional things:
- add the ability to start client without ip-address ✅
- add commands to connect, disconnect and help ✅
- add status bar (to show connected/disconnected for now) ✅
- make service messages in chat colorful ✅


## To fix
  - unwraps in senders
  - i have 2 fields that both mean "client connected": status and stream. I should get rid of status.
  - need to make widget for messages scrollable to show only last N messages if count is more than height of the area! ✅


## Start instructions

### Server
```console
$ cargo run --bin server
Token: <generated token>
<logs>
```
### Client
```console
$ cargo run --bin client 127.0.0.1
<paste or type token from server>
<type messages>
```
