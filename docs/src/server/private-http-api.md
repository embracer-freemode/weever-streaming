# Private HTTP API

There is one **private** HTTP server in Weever Streaming.
It servers for management. You can use these API to manage the room from other services.


| HTTP | Endpoint           | Usage                                                                 |
| ---- | ------------------ | --------------------------------------------------------------------- |
| POST | `/create/pub`      | Set authentication token<br />for Publisher for `<room>` for `<id>`   |
| POST | `/create/sub`      | Set authentication token<br /> for Subscriber for `<room>` for `<id>` |
| GET  | `/list/pub/<room>` | Show publisher list of `<room>`                                       |
| GET  | `/list/sub/<room>` | Show subscriber list of `<room>`                                      |
| GET  | `/metrics`         | Prometheus metrics                                                    |
