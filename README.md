Horizontal Scaling WebRTC SFU
========================================


WebRTC specs
========================================

* [RFC 8830 - WebRTC MediaStream Identification in the Session Description Protocol](https://datatracker.ietf.org/doc/rfc8830/)
    - msid

* [RFC 7478 - Web Real-Time Communication Use Cases and Requirements](https://datatracker.ietf.org/doc/rfc7478/)

* [WebRTC-HTTP ingestion protocol (WHIP)](https://datatracker.ietf.org/doc/draft-ietf-wish-whip/)
    - HTTP based signaling to create "sendonly" PeerConnection
    - HTTP POST to send SDP offer, get SDP answer in response
    - HTTP DELETE to teardown session
    - Authentication and authorization via Bearer tokens
    - Trickle ICE and ICE restart via HTTP PATCH and SDP fragments
    - [WHIP and Janus @ IIT-RTC 2021](https://www.slideshare.net/LorenzoMiniero/whip-and-janus-iitrtc-2021)



Features
========================================

* [X] audio codec: Opus
* [X] video codec: VP8
* [X] RTP BUNDLE
* [X] RTCP mux
* [X] Multistream (1 connnetion for multiple video/audio streams)
* [X] Datachannel
* [X] WebRTC Renegotiation
* [X] case: new publisher join, subscriber can get new streams
* [X] case: publisher leave, subscriber can know and delete stuffs
* [X] case: subscriber join in the middle, can get existing publishers' streams
* [ ] case: publisher leave and rejoin again
