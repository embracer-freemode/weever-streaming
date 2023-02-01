# WHIP & WHEP

## WHIP (WebRTC-HTTP Ingestion Protocol)

The [WHIP](https://datatracker.ietf.org/doc/draft-ietf-wish-whip/)
is a spec for pushing WebRTC-based media into services.
It provides a spec for WebRTC signaling with HTTP protocol and Bearer Token in pushing to services scenario.


Weever Streaming has WHIP-like media ingress.
It's WHIP-"like" because there is no guarantee about spec compliance at the moment.
But this project learned the idea from there.


And it's possible to use WHIP clients to publish audio/video to Weever Streaming.

WHIP Clients:

* [simple-whip-client](https://github.com/meetecho/simple-whip-client) (GStreamer based)
    - `./whip-client -u https://localhost:8443/pub/testroom/user1 -t mytoken -V "videotestsrc is-live=true pattern=smpte ! videoconvert ! video/x-raw,width=1280,height=720,framerate=60/1 ! queue ! vp8enc deadline=1 ! rtpvp8pay pt=96 ssrc=2 ! queue ! application/x-rtp,media=video,encoding-name=VP8,payload=96"`


## WHEP (WebRTC-HTTP Egress Protocol)

The [WHEP](https://datatracker.ietf.org/doc/draft-murillo-whep/)
is a spec for pulling WebRTC-based media from services.
It provides a spec for WebRTC signaling with HTTP protocol and Bearer Token in pulling from services scenario.
(The pulling version of WHIP.)

Weever Streaming has WHEP-like media egress.
It's WHEP-"like" because there is no guarantee about spec compliance at the moment.
Weever Streaming implemented similar idea before there is WHEP spec release.

And it's possible to use WHEP clients to pull audio/video from Weever Streaming.
But WHEP doesn't include renegotiation, so WHEP clients can't update media on the fly.

WHEP Clients:

* [simple-whep-client](https://github.com/meetecho/simple-whep-client) (GStreamer based)
    - `./whep-client -u https://localhost:8443/sub/testroom/user2 -t mytoken`
