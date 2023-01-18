Weever Streaming: Cloud Native, Horizontal Scaling, SFU
=======================================================

[![Build status](https://github.com/embracer-freemode/weever-streaming/actions/workflows/rust-check.yml/badge.svg)](https://github.com/embracer-freemode/weever-streaming/actions)

Weever Streaming is a Open Source WebRTC SFU (Selective Forwarding Unit).
It serves for broadcasting, video conferencing, ... etc.
It's easy to deploy and scalable.



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


Special Thanks
========================================

Thank [Janus Gateway](https://github.com/meetecho/janus-gateway).
I learned a lot from this project!
