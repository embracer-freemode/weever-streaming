WebRTC SFU: Cloud Native, Horizontal Scaling
============================================

A WebRTC SFU (Selective Forwarding Unit) server aim to be horizontal scalable.


Goal
========================================

The original goal is to have a WebRTC SFU server that's easy to run on Kubernetes.
And it doesn't need complex control for scaling.
Kubernetes can add more SFU pods based on loading, and things just work.
Clients can connect to any pod, they don't need to be in same instance.


Features
========================================

* multistream: one connection for subscriber for any amount of incoming streams
* [WHIP](https://datatracker.ietf.org/doc/draft-ietf-wish-whip/)-like media ingress
    - it's WHIP-"like" because there is no guarantee about spec compliance
    - but this project learned the idea from there
* [WHEP](https://datatracker.ietf.org/doc/draft-murillo-whep/)-like media egress
    - it's WHEP-"like" because there is no guarantee about spec compliance
    - this project implemented similar idea before there is WHEP spec release
* shared internal states across SFU instances (currently via Redis)
* internal traffic routing across SFU instances (currently via NATS)


Try It
========================================

```sh
git clone https://github.com/aioniclabs/webrtc-sfu
cd webrtc-sfu

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

# visit website:
#
# * https://localhost:8443/
# * https://localhost:8444/
# * https://localhost:8445/
#
# The default demo site is using self signed certs, so you need to ignore the warning in browser.
```



Deployment
========================================

See [deployment docs](./docs/deployment.md)


Special Thanks
========================================

Thank [Janus Gateway](https://github.com/meetecho/janus-gateway).
I learned a lot from this project!
