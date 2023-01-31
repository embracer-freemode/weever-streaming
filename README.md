Weever Streaming: Cloud Native, Horizontal Scaling, WebRTC SFU
==============================================================

[![Build status](https://github.com/embracer-freemode/weever-streaming/actions/workflows/rust-check.yml/badge.svg)](https://github.com/embracer-freemode/weever-streaming/actions)

Weever Streaming is a Open Source WebRTC SFU (Selective Forwarding Unit).
It serves for broadcasting, video conferencing, or regular video/audio/data streaming ... etc. It's easy to deploy and scale.

You can come up with your own streaming products/platforms easily with it, as long as you are familiar with html, css, javascript, and web service hosting. Client examples can be found at [site/index.html](https://github.com/embracer-freemode/weever-streaming/blob/develop/site/index.html) and [site/management.html](https://github.com/embracer-freemode/weever-streaming/blob/develop/site/management.html).

Currently, Weever Streaming can be deployed with docker-compose in a single machine, or with docker swarm mode to be running on multiple machines. Last but not the least, it can also be deployed to kubernetes shall you wish to go totally cloud native.


Similar or Related Projects We Know of
========================================

* [Janus](https://janus.conf.meetecho.com/): the general purpose WebRTC server
* [Jitsi](https://jitsi.org/): Video Conferencing Software


When we created Weever Streaming,
most of the popular WebRTC SFU projects scale by "room".
Different video room can be in different instance,
but all the clients in same room must connect to same instance.
They are not easily horizontal scalable, and sometime needs management works.

Janus supports wide range features. But the horizontal scaling part was a little bit of pain.
Before Janus v1.0, horizontal scaling needs to be done via RTP forwards + Streaming mountpoint.
It needs some management works.
After Janus v1.1, new APIs for remote publisher are added.
However, it still need some management, and more complex than Weever Streaming.

Jitsi comes with good and deep video conferencing integration.
But it was a bit hard to tailor it for our own applications last time we checked.
Jitsi without Octo (it's called relays now, cascaded bridges), can't easily do horizontal scaling.
It needs to deploy with sharding architecture.
After Jitsi added Octo/Relays, the streams can be routed between instances.
However, it's still a complex solution. It's more complex than we need.

Weever Streaming is released with MIT and Apache V2, with only 2 html/js code examples.
Hooking the code into your own UI and integrating into your own `jwt` auth logic is also straightforward.


Features
========================================

* 1 HTTP POST for connection setup
* subscriber multistream
    - 1 connection for any amount of stream subscribing (`O(1)` port usage)
    - publisher is 1 connection for each stream publishing (`O(N)` port usage)
* (optional) authentication via Bearer Token
* [WHIP](https://datatracker.ietf.org/doc/draft-ietf-wish-whip/)-like media ingress
    - it's WHIP-"like" because there is no guarantee about spec compliance
    - but this project learned the idea from there
* [WHEP](https://datatracker.ietf.org/doc/draft-murillo-whep/)-like media egress
    - it's WHEP-"like" because there is no guarantee about spec compliance
    - this project implemented similar idea before there is WHEP spec release
* shared internal states across SFU instances (currently via Redis, can be extended)
* internal traffic routing across SFU instances (currently via NATS, can be extended)


Try It
========================================

```sh
git clone https://github.com/embracer-freemode/weever-streaming
cd weever-streaming

# you need to install "docker-compose" first
# this will launch: 1 Redis, 3 NATS, 3 WebRTC SFU
#
#   ┌──────┐    ┌──────┐    ┌──────┐
#   │ SFU1 ├──┐ │ SFU2 ├──┐ │ SFU3 ├──┐
#   └───┬──┘  │ └───┬──┘  │ └───┬──┘  │
#       │     │     │     │     │     │
#   ┌───▼───┐ │ ┌───▼───┐ │ ┌───▼───┐ │
#   │ NATS1 ◄─┼─► NATS2 ◄─┼─► NATS3 │ │
#   └───────┘ │ └───────┘ │ └───────┘ │
#             │           │           │
#   ┌─────────▼───────────▼───────────▼──┐
#   │               Redis                │
#   └────────────────────────────────────┘
#
docker-compose up

# visit website (publisher & subscriber can be in different instance):
#
# * https://localhost:8443/
# * https://localhost:8444/
# * https://localhost:8445/
#
# The default demo site is using self signed certs, so you need to ignore the warning in browser.
```


[![SFU demo](https://user-images.githubusercontent.com/2716047/209774978-aba37989-dca9-427e-8519-821d2cd16790.mp4)](https://user-images.githubusercontent.com/2716047/209774978-aba37989-dca9-427e-8519-821d2cd16790.mp4)


Deployment
========================================

* [Docker Compose](./docs/deploy/docker-compose.md)
* [Docker Swarm](./docs/deploy/docker-swarm.md)
* [Kubernetes](./docs/deploy/kubernetes.md)


API
========================================

Public HTTP API
------------------------------

| HTTP | Endpoint           | Usage                                               |
| ---- | ------------------ | --------------------------------------------------- |
| POST | `/pub/<room>/<id>` | Connect WebRTC as Publisher for `<room>` as `<id>`  |
| POST | `/sub/<room>/<id>` | Connect WebRTC as Subscriber for `<room>` as `<id>` |


Private HTTP API
------------------------------

| HTTP | Endpoint           | Usage                                                           |
| ---- | ------------------ | --------------------------------------------------------------- |
| POST | `/create/pub`      | Set authentication token for Publisher for `<room>` for `<id>`  |
| POST | `/create/sub`      | Set authentication token for Subscriber for `<room>` for `<id>` |
| GET  | `/list/pub/<room>` | Show publisher list of `<room>`                                 |
| GET  | `/list/sub/<room>` | Show subscriber list of `<room>`                                |


Development
========================================

Compile
------------------------------

```sh
cargo build
```

CI
------------------------------

[GitHub Actions](https://github.com/embracer-freemode/weever-streaming/actions)


TODOs
========================================

* [ ] an awesome basic UI for user to come and get impressed
* [ ] a video tutorial for each deployment method
* [ ] beef up the doc for Kubernetes deployment


Special Thanks
========================================

Thank [Janus Gateway](https://github.com/meetecho/janus-gateway).
I learned a lot from this project!
