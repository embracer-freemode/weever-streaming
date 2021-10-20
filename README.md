Horizontal Scaling WebRTC SFU
========================================



Roadmap
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



WebRTC specs
========================================

* [RTCWEB working group](https://datatracker.ietf.org/wg/rtcweb/about/)
* [Web Real-Time Communications Working Group Charter](https://w3c.github.io/webrtc-charter/webrtc-charter.html)
* [W3C - WebRTC 1.0: Real-Time Communication Between Browsers](https://www.w3.org/TR/webrtc/)
* [W3C - WebRTC Next Version Use Cases](https://www.w3.org/TR/webrtc-nv-use-cases/)
* [W3C - Scalable Video Coding (SVC) Extension for WebRTC](https://www.w3.org/TR/webrtc-svc/)
* [W3C - Screen Capture](https://www.w3.org/TR/screen-capture/)
* [W3C - MediaStreamTrack Content Hints](https://www.w3.org/TR/mst-content-hint/)
* [W3C - Viewport Capture](https://w3c.github.io/mediacapture-viewport/)
* [W3C - WebRTC Encoded Transform](https://w3c.github.io/webrtc-encoded-transform/)
* [W3C - Media Capture and Streams Extensions](https://w3c.github.io/mediacapture-extensions/)
* [W3C - Identifiers for WebRTC's Statistics API](https://www.w3.org/TR/webrtc-stats/)
* [W3C - Identity for WebRTC 1.0](https://www.w3.org/TR/webrtc-identity/)
* [W3C - Audio Output Devices API](https://www.w3.org/TR/audio-output/)
* [W3C - Media Capture from DOM Elements](https://www.w3.org/TR/mediacapture-fromelement/)
* [W3C - MediaStream Recording](https://www.w3.org/TR/mediastream-recording/)
* [W3C - WebRTC Priority Control API](https://www.w3.org/TR/webrtc-priority/)
* [W3C - IceTransport Extensions for WebRTC](https://w3c.github.io/webrtc-ice/)
* [W3C - WebRTC 1.0 Interoperability Tests Results](https://w3c.github.io/webrtc-interop-reports/webrtc-pc-report.html)
* [RFC 7478 - Web Real-Time Communication Use Cases and Requirements](https://datatracker.ietf.org/doc/rfc7478/)
* [RFC 7742 - WebRTC Video Processing and Codec Requirements](https://datatracker.ietf.org/doc/rfc7742/)
* [RFC 7874 - WebRTC Audio Codec and Processing Requirements](https://datatracker.ietf.org/doc/rfc7874/)
* [RFC 7875 - Additional WebRTC Audio Codecs for Interoperability](https://datatracker.ietf.org/doc/rfc7875/)
* [RFC 8451 - Considerations for Selecting RTP Control Protocol (RTCP) Extended Report (XR) Metrics for the WebRTC Statistics API](https://datatracker.ietf.org/doc/rfc8451/)
* [RFC 8826 - Security Considerations for WebRTC](https://datatracker.ietf.org/doc/rfc8826/)
* [RFC 8827 - WebRTC Security Architecture](https://datatracker.ietf.org/doc/rfc8827/)
* [RFC 8828 - WebRTC IP Address Handling Requirements](https://datatracker.ietf.org/doc/rfc8828/)
* [RFC 8830 - WebRTC MediaStream Identification in the Session Description Protocol](https://datatracker.ietf.org/doc/rfc8830/)
* [RFC 8831 - WebRTC Data Channels](https://datatracker.ietf.org/doc/rfc8831/)
    - msid
* [RFC 8832 - WebRTC Data Channel Establishment Protocol](https://datatracker.ietf.org/doc/rfc8832/)
* [RFC 8833 - Application-Layer Protocol Negotiation (ALPN) for WebRTC](https://datatracker.ietf.org/doc/rfc8833/)
* [RFC 8834 - Media Transport and Use of RTP in WebRTC](https://datatracker.ietf.org/doc/rfc8834/)
* [RFC 8835 - Transports for WebRTC](https://datatracker.ietf.org/doc/rfc8835/)
* [RFC 8837 - Differentiated Services Code Point (DSCP) Packet Markings for WebRTC QoS](https://datatracker.ietf.org/doc/rfc8837/)
* [RFC 8854 - WebRTC Forward Error Correction Requirements](https://datatracker.ietf.org/doc/rfc8854/)
* [RFC 8865 - T.140 Real-Time Text Conversation over WebRTC Data Channels](https://datatracker.ietf.org/doc/rfc8865/)
* [draft-ietf-rtcweb-sdp-14 - Annotated Example SDP for WebRTC](https://datatracker.ietf.org/doc/draft-ietf-rtcweb-sdp/)

* [WebRTC-HTTP ingestion protocol (WHIP)](https://datatracker.ietf.org/doc/draft-ietf-wish-whip/)
    - HTTP based signaling to create "sendonly" PeerConnection
    - HTTP POST to send SDP offer, get SDP answer in response
    - HTTP DELETE to teardown session
    - Authentication and authorization via Bearer tokens
    - Trickle ICE and ICE restart via HTTP PATCH and SDP fragments
    - [WHIP and Janus @ IIT-RTC 2021](https://www.slideshare.net/LorenzoMiniero/whip-and-janus-iitrtc-2021)
    - [WISH (WebRTC Ingest Signaling over HTTPS) working group](https://datatracker.ietf.org/wg/wish/about/)
