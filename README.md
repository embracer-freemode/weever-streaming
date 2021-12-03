Cloud Native, Horizontal Scaling WebRTC SFU
===========================================

A WebRTC SFU (Selective Forwarding Unit) server aim to be horizontal scalable.

This project combine the experience in multiple fields,
including state-of-the-art WebRTC flow (e.g. multistream, WHIP), Kubernetes deployment, Rust development.

You can view the table of content on GitHub like this:
![github-toc](https://user-images.githubusercontent.com/2716047/132623287-c276e4a0-19a8-44a5-bbdb-41e1cdc432e1.gif)


High Level Idea
========================================

Problem
------------------------------

The [Janus](https://janus.conf.meetecho.com) server can't easily scale horizontally.
It needs either manually media forward setup for publishers/subscribers,
or use it in provision style (prepare big enough resource for an event).

And the room creation management in Janus is not very smooth.
If the instance is restarted, you need to config the room again.
There is no builtin way to easily restore the states.


Solution
------------------------------

Options:

1. write custom Janus plugin to meet our goal
2. survey other media servers to see if they can horizontal scale
3. write whole WebRTC SFU server by ourself (full control, but need more development)


This project picks solution #3.
We write our own WebRTC SFU server.
We built the server with horizontal scale in mind.


Pros:
* full control for the media
* can customize whatever we want

Cons:
* need more development time
* need deeper media knowledge



SFU Design
========================================

1 HTTP POST to connect
------------------------------

There is no complex negotiation and setting.
We use single HTTP POST request with SDP offer from client to connect WebRTC.
Server will provide SDP answer in HTTP response.
Then following communication will based on the data channel.

This means browser will always be the offer side, server will always be the answer side.
Even in subscriber case, browser will connect with a single data channel first as offer side.
Then update the media based on info from data channel.


Connection first, renegotiate media later
-----------------------------------------

We choose to setup WebRTC connection first.
Don't care too much about how many media we want.
We start WebRTC connection with only 1 data channel.
After the WebRTC become connected,
server will send current room's media info via data channel.
Then we setup the media flow via WebRTC renegotiation.


Every PeerConnection will use unique id
---------------------------------------

WebRTC's track stop/resume/...etc are a bit complex to control.
And reuse RTP sender with replace track has rendering state problem.
To simplify the case, we use unique id for every PeerConnection.
Same user will use a random string as suffix everytime the client connects.
So the clients are actually all different. No reuse problem.

However, this come with the cost that a subscriber in the room will gradually have bigger SDP when publishers come and go.


Data Channel Commands List
------------------------------

Frontend can use data channel to update SDP.
So we can use WebRTC renegotiation to get new video/audio.

Data Channel command list:

* server to browser
    - `PUB_JOIN <ID>`: let frontend know a publisher just join
    - `PUB_LEFT <ID>`: let frontend know a publisher just left
    - `SDP_OFFER <SDP>`: new SDP offer, browser should reply SDP_ANSWER
* browser to server
    - `SDP_ANSWER <SDP>`: new SDP answer, server should then finish current renegotiation
    - `STOP`: tell server to close WebRTC connection and cleanup related stuffs


Room States Sharing
------------------------------

Current room states are shared across pods via **Redis**.
So newly created pod can grab the settings from Redis when needed.
There is no need for other room preparation.

Shared info includes:
* list of publisher in specific room
* list of subscriber in specific room
* per publisher video/audio count


Cross Pods Internal Commands
------------------------------

Internal commands are serialize/deserialize via bincode and send via **NATS**.
All the commands are collected in an enum called `Command`.

This includes:
* PubJoin
* PubLeft



WebRTC specs
========================================

* [RTCWEB working group](https://datatracker.ietf.org/wg/rtcweb/about/)
* [Web Real-Time Communications Working Group Charter](https://w3c.github.io/webrtc-charter/webrtc-charter.html)
* [W3C - WebRTC 1.0: Real-Time Communication Between Browsers](https://www.w3.org/TR/webrtc/)
* [W3C - WebRTC Next Version Use Cases](https://www.w3.org/TR/webrtc-nv-use-cases/)
    - multiparty online game with voice communications
    - mobile calling service
    - video conferencing with a central server
    - file sharing
    - internet of things
    - funny hats effect
    - machine learning
    - virtual reality gaming
    - requirements
        + N01: ICE candidates control, e.g. gathering and pruning
        + N02: multiple connections with one offer
        + N03: congestion control for audio quality and latency betweeen multiple connections
        + N04: move traffic between multiple ICE candidates
        + N05: ICE candidates will consider network cost when doing re-routing
        + ...
* [W3C - Scalable Video Coding (SVC) Extension for WebRTC](https://www.w3.org/TR/webrtc-svc/)
    - SST (Single-Session Transmission)
    - MST (Multi-Session Transmission)
    - MRST (Multiple RTP stream Single Transport)
    - Spatial Simulcast and Temporal Scalability
        + "L2T3" means 2 Spatial Layers & 3 Temporal Layers & Inter-layer dependency
        + "S2T3" means 2 Spatial Layers & 3 Temporal Layers & No Inter-layer dependency (Simulcast)
    - `scalabilityMode`
* [W3C - Screen Capture](https://www.w3.org/TR/screen-capture/)
    - `navigator.mediaDevices.getDisplayMedia`
* [W3C - MediaStreamTrack Content Hints](https://www.w3.org/TR/mst-content-hint/)
    - audio: "", "speech", "speech-recognition", "music"
    - video: "", "motion", "detail", "text"
    - degradation preference: "maintain-framerate", "maintain-resolution", "balanced"
* [W3C - Viewport Capture](https://w3c.github.io/mediacapture-viewport/)
* [W3C - WebRTC Encoded Transform](https://w3c.github.io/webrtc-encoded-transform/)
    - manipulating the bits on MediaStreamTracks being sent via an RTCPeerConnection
    - e.g. funny hats effect, machine learning model, virtual reality
* [W3C - Media Capture and Streams Extensions](https://w3c.github.io/mediacapture-extensions/)
    - semantics: "browser-chooses", "user-chooses"
    - deviceId
* [W3C - Identifiers for WebRTC's Statistics API](https://www.w3.org/TR/webrtc-stats/)
    - types
        + codec
        + inbound-rtp
        + outbound-rtp
        + remote-inbound-rtp
        + remote-outbound-rtp
        + media-source
        + csrc
        + peer-connection
        + data-channel
        + stream
        + track
        + transceiver
        + sender
        + receiver
        + transport
        + sctp-transport
        + candidate-pair
        + local-candidate
        + remote-candidate
        + certificate
        + ice-server
    - RTCP Receiver Report (RR)
    - RTCP Extended Report (XR)
    - RTCP Sender Report (SR)
    - audio `voiceActivityFlag`: Whether the last RTP packet whose frame was delivered to the RTCRtpReceiver's MediaStreamTrack for playout contained voice activity or not based on the presence of the V bit in the extension header, as defined in RFC6464. This is the stats-equivalent of RTCRtpSynchronizationSource.
    - RTCQualityLimitationReason
* [W3C - Identity for WebRTC 1.0](https://www.w3.org/TR/webrtc-identity/)
* [W3C - Audio Output Devices API](https://www.w3.org/TR/audio-output/)
* [W3C - Media Capture from DOM Elements](https://www.w3.org/TR/mediacapture-fromelement/)
* [W3C - MediaStream Recording](https://www.w3.org/TR/mediastream-recording/)
* [W3C - WebRTC Priority Control API](https://www.w3.org/TR/webrtc-priority/)
    - WebRTC uses the priority and Quality of Service (QoS) framework described in rfc8835 and rfc8837 to provide priority and DSCP marking for packets that will help provide QoS in some networking environments.
* [W3C - IceTransport Extensions for WebRTC](https://w3c.github.io/webrtc-ice/)
* [W3C - WebRTC 1.0 Interoperability Tests Results](https://w3c.github.io/webrtc-interop-reports/webrtc-pc-report.html)
* [RFC 4566 - SDP: Session Description Protocol](https://datatracker.ietf.org/doc/rfc4566/)
    - example usage: SIP (Session Initiation Protocol), RTSP (Real Time Streaming Protocol), SAP (Session Announcement Protocol)
    - `<type>=<value>`, `<type>` MUST be exactly one case-significant character and `<value>` is structured text whose format depends on `<type>`
    - Whitespace MUST NOT be used on either side of the "=" sign
    - Session description
        + v=  (protocol version)
        + o=  (originator and session identifier)
        + s=  (session name)
        + i=* (session information)
        + u=* (URI of description)
        + e=* (email address)
        + p=* (phone number)
        + c=* (connection information -- not required if included in all media)
        + b=* (zero or more bandwidth information lines)
        + One or more time descriptions ("t=" and "r=" lines; see below)
        + z=* (time zone adjustments)
        + k=* (encryption key)
        + a=* (zero or more session attribute lines)
        + Zero or more media descriptions
    - Time description
        + t=  (time the session is active)
        + r=* (zero or more repeat times)
    - Media description, if present
        + m=  (media name and transport address)
        + i=* (media title)
        + c=* (connection information -- optional if included at session level)
        + b=* (zero or more bandwidth information lines)
        + k=* (encryption key)
        + a=* (zero or more media attribute lines)
* [RFC 5245 - Interactive Connectivity Establishment (ICE): A Protocol for Network Address Translator (NAT) Traversal for Offer/Answer Protocols](https://datatracker.ietf.org/doc/rfc5245/)
* [RFC 5285 - A General Mechanism for RTP Header Extensions](https://datatracker.ietf.org/doc/rfc5285/)
* [RFC 6386 - VP8 Data Format and Decoding Guide](https://datatracker.ietf.org/doc/rfc6386/)
* [RFC 6716 - Definition of the Opus Audio Codec](https://datatracker.ietf.org/doc/rfc6716/)
    - based on LPC (Linear Predictive Coding) & MDCT (Modified Discrete Cosine Transform)
    - in speech, LPC based (e.g. CELP) code low frequencies more efficiently than transform domain techniques (e.g. MDCT)
    - The Opus codec includes a number of control parameters that can be changed dynamically during regular operation of the codec, without interrupting the audio stream from the encoder to the decoder.
    - These parameters only affect the encoder since any impact they have on the bitstream is signaled in-band such that a decoder can decode any Opus stream without any out-of-band signaling.
    - Any Opus implementation can add or modify these control parameters without affecting interoperability.
    - Control Parameters
        + Bitrate (6 ~ 510 kbit/s)
        + Number of Channels (Mono/Stereo)
        + Audio Bandwidth
        + Frame Duration
        + Complexity (CPU complexity v.s. quality/bitrate)
        + Packet Loss Resilience
        + Forward Error Correction (FEC)
        + Constant/Variable Bitrate
        + Discontinuous Transmission (DTX) (can reduce the bitrate during silence or background noise)
* [RFC 7478 - Web Real-Time Communication Use Cases and Requirements](https://datatracker.ietf.org/doc/rfc7478/)
* [RFC 7587 - RTP Payload Format for the Opus Speech and Audio Codec](https://datatracker.ietf.org/doc/rfc7587/)
* [RFC 7667 - RTP Topologies](https://datatracker.ietf.org/doc/rfc7667/)
* [RFC 7741 - RTP Payload Format for VP8 Video](https://datatracker.ietf.org/doc/rfc7741/)
* [RFC 7742 - WebRTC Video Processing and Codec Requirements](https://datatracker.ietf.org/doc/rfc7742/)
* [RFC 7874 - WebRTC Audio Codec and Processing Requirements](https://datatracker.ietf.org/doc/rfc7874/)
* [RFC 7875 - Additional WebRTC Audio Codecs for Interoperability](https://datatracker.ietf.org/doc/rfc7875/)
* [RFC 8451 - Considerations for Selecting RTP Control Protocol (RTCP) Extended Report (XR) Metrics for the WebRTC Statistics API](https://datatracker.ietf.org/doc/rfc8451/)
* [RFC 8826 - Security Considerations for WebRTC](https://datatracker.ietf.org/doc/rfc8826/)
* [RFC 8827 - WebRTC Security Architecture](https://datatracker.ietf.org/doc/rfc8827/)
* [RFC 8828 - WebRTC IP Address Handling Requirements](https://datatracker.ietf.org/doc/rfc8828/)
* [RFC 8829 - JavaScript Session Establishment Protocol (JSEP)](https://datatracker.ietf.org/doc/rfc8829/)
    - PeerConnection.addTrack
        + if the PeerConnection is in the "have-remote-offer" state, the track will be attached to the first compatible transceiver that was created by the most recent call to setRemoteDescription and does not have a local track.
        + Otherwise, a new transceiver will be created
    - PeerConnection.removeTrack
        + The sender's track is cleared, and the sender stops sending.
        + Future calls to createOffer will mark the "m=" section associated with the sender as recvonly (if transceiver.direction is sendrecv) or as inactive (if transceiver.direction is sendonly).
    - RtpTransceiver
        + RtpTransceivers allow the application to control the RTP media associated with one "m=" section.
        + Each RtpTransceiver has an RtpSender and an RtpReceiver, which an application can use to control the sending and receiving of RTP media.
        + The application may also modify the RtpTransceiver directly, for instance, by stopping it.
        + RtpTransceivers can be created explicitly by the application or implicitly by calling setRemoteDescription with an offer that adds new "m=" sections.
    - RtpTransceiver.stop
        + The stop method stops an RtpTransceiver.
        + This will cause future calls to createOffer to generate a zero port for the associated "m=" section.
    - Subsequent Offers
        + If any RtpTransceiver has been added and there exists an "m=" section with a zero port in the current local description or the current remote description, that "m=" section MUST be recycled by generating an "m=" section for the added RtpTransceiver as if the "m=" section were being added to the session description (including a new MID value) and placing it at the same index as the "m=" section with a zero port.
        + If an RtpTransceiver is stopped and is not associated with an "m=" section, an "m=" section MUST NOT be generated for it.
        + If an RtpTransceiver has been stopped and is associated with an "m=" section, and the "m=" section is not being recycled as described above, an "m=" section MUST be generated for it with the port set to zero and all "a=msid" lines removed.

* [RFC 8830 - WebRTC MediaStream Identification in the Session Description Protocol](https://datatracker.ietf.org/doc/rfc8830/)
    - grouping mechanism for RTP media streams
    - MediaStreamTrack: unidirectional flow of media data (either audio or video, but not both). One MediaStreamTrack can be present in zero, one, or multiple MediaStreams. (source stream)
    - MediaStream: assembly of MediaStreamTracks, can be different types.
    - Media description: a set of fields starting with an "m=" field and terminated by either the next "m=" field or the end of the session description.
    - each RTP stream is distinguished inside an RTP session by its Synchronization Source (SSRC)
    - each RTP session is distinguished from all other RTP sessions by being on a different transport association (2 transport assertions if no RTP/RTCP multiplexing)
    - if multiple media sources are carried in an RTP session, this is signaled using BUNDLE
    - RTP streams are grouped into RTP sessions and also carry a CNAME
    - Neither CNAME nor RTP session corresponds to a MediaStream, the association of an RTP stream to MediaStreams need to be explicitly signaled.
    - MediaStreams identifier (msid) associates RTP streams that are described in separate media descriptions with the right MediaStreams
    - the value of the "msid" attribute consists of an identifier and an optional "appdata" field
    - `msid-id [ SP msid-appdata ]`
    - There may be multiple "msid" attributes in a single media description. This represents the case where a single MediaStreamTrack is present in multiple MediaStreams.
    - Endpoints can update the associations between RTP streams as expressed by "msid" attributes at any time.
* [RFC 8831 - WebRTC Data Channels](https://datatracker.ietf.org/doc/rfc8831/)
    - SCTP over DTLS over UDP
    - have both Reliable and Unreliable mode
    - U-C 6: Renegotiation of the configuration of the PeerConnection
    - WebRTC data channel mechanism does not support SCTP multihoming
    - in-order or out-of-order
    - the sender should disable the Nagle algorithm to minimize the latency
* [RFC 8832 - WebRTC Data Channel Establishment Protocol](https://datatracker.ietf.org/doc/rfc8832/)
    - DCEP (Data Channel Establishment Protocol)
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



Rust Resources
========================================

Books and Docs:

* [The Rust Programming Language](https://doc.rust-lang.org/book/) (whole book online)
* [Rust by Example](https://doc.rust-lang.org/rust-by-example/)
* [The Cargo Book](https://doc.rust-lang.org/cargo/) (the package manager)
* [Standard Library](https://doc.rust-lang.org/stable/std/index.html)
* [Docs.rs](https://docs.rs) (API docs for whatever crate published in ecosystem)
* [crates.io](https://crates.io) (Rust packages center)
* [rust-analyzer](https://rust-analyzer.github.io) (Language Server Protocol support for the Rust, highly recommended, it provides many info for your editor)
* [Tokio](https://docs.rs/tokio/latest/) (the async runtime)
* [Serde](https://serde.rs) (serialize/deserialize framework)
* [Clap](https://clap.rs) (CLI interface, including env loading)
* [WebRTC.rs](https://github.com/webrtc-rs/webrtc)


Also you can build documentation site via command:

```sh
cargo doc
```

Open with your default browser

```sh
# URL will look like "file:///path/to/project/target/doc/project/index.html"
cargo doc --open
```


This will have API level overview of what's in the codebase.
And you will get all the API docs from dependencies.



How to build project
========================================

Code checking
------------------------------

This only perform types checking, lifetime checking, ... etc.
But it won't ask compiler to generate binary.
This will be faster than ``cargo build``,
which suit well for development cycle.

```sh
cargo check
```


Development build
------------------------------

```sh
cargo build
```


Release build
------------------------------

Release build will take more time than development build.

```sh
cargo build --release
```


Container build
------------------------------

Related setup all live in ``Dockerfile``

```sh
docker build .
```


Run
========================================

Environment prepare
------------------------------

Environment dependencies:
* [NATS server](https://nats.io) (for distributing media)
* [Redis](https://redis.io) (for sharing room metadata)


Run the program
------------------------------

Run with default options:

```sh
cargo run
```


Run with customize options:

```sh
cargo run -- ...
```


Changing log level:

```sh
env RUST_LOG=info cargo run -- ...
```


Checking what arguments (and environment variables) can use:
(if the CLI argument is not present, env variable will be used as fallback)

```sh
cargo run -- --help
```
![cargo-cli](https://user-images.githubusercontent.com/2716047/138387972-14e193b0-cde1-47b6-af20-374cbda5a234.png)



Testing
========================================

Doctest
------------------------------

(TODO: not implmented for this repo right now)

You can write some code samples and assertions in doc comments style.
Those cases will be discovered and run.
Here are some [samples](https://doc.rust-lang.org/rust-by-example/testing/doc_testing.html).


Unit Test
------------------------------

(TODO: not implmented for this repo right now)

You can put your test case into the source code file with ``#[cfg(test)]``.
Those cases will be discovered and run.
Here are some [samples](https://doc.rust-lang.org/rust-by-example/testing/unit_testing.html).


Integration Test
------------------------------

(TODO: not implmented for this repo right now)

The test cases are in the ``tests`` folder.
Here are some [samples](https://doc.rust-lang.org/rust-by-example/testing/integration_testing.html).

You can run the testing with this command:

```sh
cargo test
```

If you need coverage report, you can install [Tarpaulin](https://github.com/xd009642/tarpaulin).
Then run:

```sh
# default is showing coverage report in stdout
# "-o Html" will generate a HTML file so that you can have more browsing
cargo tarpaulin -o Html
```


Fuzz Test
------------------------------

(TODO: not implmented for this repo right now)

You can add some fuzz testing to uncover some cases that's not included in existing test cases.
With engine like AFL or LLVM libFuzzer.
Here is the [guide for fuzz](https://rust-fuzz.github.io/book/introduction.html).



Benchmark
========================================

(TODO: not implmented for this repo right now)

It's definitely good idea to write bechmark before you tweak performance.
There is builtin [cargo bench](https://doc.rust-lang.org/cargo/commands/cargo-bench.html) support.
If you want more advanced features,
[Criterion](https://bheisler.github.io/criterion.rs/book/getting_started.html)
is good one with more charting support.



How to update dependencies
========================================

The dependencies are written in ``Cargo.toml``.
And the ``Cargo.lock`` is the version lock for everthing being used.

If you want to see if there is new version in ecosystem,
you can run:

```
cargo update --dry-run
```



CI (Continuous Integration)
========================================

Currently, we are using CircleCI.
The config is at ``.circleci/config.yml``.

Here is the link point to [CircleCI for this project](https://app.circleci.com/pipelines/github/aioniclabs/webrtc-sfu).



Code Structure
========================================

Files
------------------------------

```sh
.
├── .circleci
│  └── config.yml # CircleCI config
├── site
│  └── index.yml  # demo website
├── Cargo.lock    # Rust package dependencies lock
├── Cargo.toml    # Rust package dependencies
├── Dockerfile    # container build setup
├── README.md
└── src           # main code
   ├── cli.rs         # CLI/env options
   ├── helper.rs      # small utils
   ├── lib.rs         # entry point
   ├── publisher.rs   # WebRTC as media publisher
   ├── state.rs       # global sharing states across instances
   ├── subscriber.rs  # WebRTC as media subscriber
   └── web.rs         # web server
```

Docs site
------------------------------

You can build API documentation site via command:

```sh
cargo doc
```


Launching Flow in Program
------------------------------

When program launches, roughly these steps will happen:
1. parse CLI args and environment variables
2. create logger
3. load SSL certs
4. create Redis client
5. create NATS client
6. run web server


Extra Learning
========================================

This project shows that:

* we can leverage existing WebRTC libraries to build our SFU server
* we can reuse whatever pub/sub pattern message bus for media distribution
* the WebRTC signaling exchange can simplify to 1 HTTP request/response



Future Works
========================================

* Media
    - [X] audio codec: Opus
    - [X] video codec: VP8
    - [X] RTP BUNDLE
    - [X] RTCP mux
    - [X] Multistream (1 connnetion for multiple video/audio streams pulling)
    - [X] Datachannel
    - [X] WebRTC Renegotiation
    - [ ] audio codec parameters config
    - [ ] video codec parameters config
    - [ ] ICE restart
    - [ ] SVC: VP8-SVC
    - [ ] SVC: AV1-SVC
    - [ ] Simulcast
    - [ ] RTX (retransmission) check
    - [ ] FEC (Forward Error Correction)
    - [ ] PLI (Picture Loss Indication) control
    - [ ] FIR (Full Intra Request)
    - [ ] NACK (Negative Acknowledgement) check
    - [ ] video codec: H264
    - [ ] video codec: VP9
    - [ ] video codec: AV1
    - [ ] Opus in-band FEC
    - [ ] Opus DTX (discontinuous transmission)
    - [ ] RTP Header Extensions check (hdrext)
    - [ ] Reduced-Size RTCP check (rtcp-rsize)
    - [ ] BWE (bandwidth estimation) / Congestion Control (goog-remb & transport-cc) check
    - [ ] bandwidth limitation in SDP
    - [ ] latency measurement
    - [ ] E2E (End to End) Encryption
    - [ ] audio mixing

* Use Cases
    - [X] new publisher join, existing subscriber can get new streams
    - [X] publisher leave, existing subscriber can know and delete stuffs
    - [X] new subscriber join in the middle, can get existing publishers' streams
    - [X] publisher leave and rejoin again
    - [X] cover all audio room use cases
    - [X] screen share (via 1 extra WebRTC connection)
    - [X] cover all video room use cases
    - [ ] chatting via datachannel

* Horizontal Scale
    - [X] shared state across instances
    - [ ] instance killed will cleanup related resource in Redis
    - [ ] subscriber based scaling mechanism (need to cowork with Helm chart)
    - [ ] Kubernetes readiness API

* Stability
    - [X] compiler warnings cleanup
    - [X] set TTL for all Redis key/value (1 day)
    - [X] don't send PUB_JOIN to subscriber if publisher is exactly the subscriber
    - [X] don't send RENEGOTIATION if subscriber is ongoing another renegotiation, hold and send later to avoid frontend state issue
    - [X] don't change transceivers if subscriber is ongoing renegotiation, hold and change later
    - [ ] make sure all Tokio tasks will end when clients leave
    - [ ] unwrap usage cleanup
    - [ ] WebRTC spec reading
    - [ ] more devices test (Windows/MacOS/Linux/Android/iOS with Chrome/Firefox/Safari/Edge)
    - [ ] test cases

* Performance Optimization
    - [X] (subscriber) don't create transceiver at first hand when publisher is the same as subscriber
    - [X] don't pull streams for subscriber, if the publisher is with same id
    - [X] enable compiler LTO
    - [X] faster showing on subscribers' site when publisher join
    - [X] reduce media count via track cleanup
    - [ ] use same WebRTC connection for screen share (media add/remove for same publisher)
    - [ ] compile with `RUSTFLAGS="-Z sanitizer=leak"` and test, make sure there is no memory leak
    - [ ] WebRTC.rs stack digging
    - [ ] guarantee connection without extra TURN?
    - [ ] support select codec & rate from client

* Monitor
    - [ ] Prometheus endpoint for per room metrics
    - [ ] use spans info to show on Grafana (by room)
    - [ ] use spans info to show on Grafana (by user)

* Demo Site
    - [X] select video resolution (e.g. 720p, 240p)
    - [X] publisher can select enable audio/video or not
    - [X] video resolution setting
    - [X] video framerate setting

* Misc
    - [X] in-cluster API for publishers list
    - [X] in-cluster API for subscribers list
    - [X] assign public IP from outside to show on the ICE (via set_nat_1to1_ips)
    - [X] show selected ICE candidate on demo site
    - [X] CORS setting
    - [ ] split user API and internal setting API
    - [ ] force non-trickle on web
    - [ ] better TURN servers setup for demo site
    - [ ] `SUB/UNSUB <PUB_ID> <APP_ID>` data channel command
    - [ ] data channel protocol ("auto", "manual"), "auto" mode means auto subscribe all publishers, "manual" mode means browser choose which to subscribe
    - [ ] `SUB_MODE <0/1>` switch between "auto"/"manual" mode
    - [ ] in "manual" mode, server auto push media list when publishers join/leave, so browser can choose
    - [ ] dynamically add video/audio in existing publisher (`ADD_MEDIA <VIDEO/AUDIO> <APP_ID>` `REMOVE_MEDIA <VIDEO/AUDIO> <APP_ID>`)

* Issues (discover during development or team test)
    - [X] sometime it needs 10 seconds to become connected (network problem?)
    - [X] we get some broken audio from time to time -> OK now
    - [X] Safari can't connect -> OK now
    - [X] publisher rejoin sometime will cause video stucking on subscriber side -> each time publisher join will use an extra random trailing string in the id

* Refactor
    - [X] redesign the SDP flow
    - [ ] share WebRTC generic part of publisher/subscriber code
    - [ ] redesign the room metadata

* Docs
    - [X] state sharing via Redis
    - [X] internal commands via NATS
    - [X] data channel command list
    - [ ] WebRTC flow explain for publisher/subscriber
