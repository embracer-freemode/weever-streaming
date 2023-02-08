# Public HTTP API

Public HTTP APIs in Weever Streaming for publisher/subscriber connection.

| HTTP | Endpoint           | Usage                                               |
| ---- | ------------------ | --------------------------------------------------- |
| POST | `/pub/<room>/<id>` | Connect WebRTC as Publisher for `<room>` as `<id>`  |
| POST | `/sub/<room>/<id>` | Connect WebRTC as Subscriber for `<room>` as `<id>` |
