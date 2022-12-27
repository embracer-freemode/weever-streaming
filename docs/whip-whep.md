# WHIP

Clients:

* [simple-whip-client](https://github.com/meetecho/simple-whip-client)
    - `./whip-client -u https://localhost:8443/pub/testroom/user1 -t mytoken -V "videotestsrc is-live=true pattern=smpte ! videoconvert ! video/x-raw,width=1280,height=720,framerate=60/1 ! queue ! vp8enc deadline=1 ! rtpvp8pay pt=96 ssrc=2 ! queue ! application/x-rtp,media=video,encoding-name=VP8,payload=96"`
