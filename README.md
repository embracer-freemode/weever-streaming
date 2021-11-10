Horizontal Scaling WebRTC SFU
========================================

A WebRTC SFU (Selective Forwarding Unit) server aim to be horizontal scalable.

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
* [RFC 4566 - SDP Session Description Protocol](https://datatracker.ietf.org/doc/rfc4566/)
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
   ├── cli.rs     # CLI/env options
   └── lib.rs     # WebRTC & web server
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


States Sharing
------------------------------

Curretly, room/publisher/subscriber settings are shared across instances via Redis.



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

* Stability
    - [X] compiler warnings cleanup
    - [ ] make sure all Tokio tasks will end when clients leave
    - [ ] set TTL for all Redis key/value
    - [ ] unwrap usage cleanup
    - [ ] WebRTC spec reading
    - [ ] more devices test (Windows/MacOS/Linux/Android/iOS with Chrome/Firefox/Safari/Edge)

* Performance Optimization
    - [ ] use same WebRTC connection for screen share (media add/remove for same publisher)
    - [ ] don't pull streams for subscriber, if the publisher is with same id
    - [ ] compile with `RUSTFLAGS="-Z sanitizer=leak"` and test, make sure there is no memory leak
    - [ ] faster showing on subscribers' site when publisher join
    - [ ] WebRTC.rs stack digging

* Monitor
    - [ ] use spans info to show on Grafana (by room)
    - [ ] use spans info to show on Grafana (by user)

* Misc
    - [X] in-cluster API for publishers list
    - [X] in-cluster API for subscribers list
    - [X] assign public IP from outside to show on the ICE (via set_nat_1to1_ips)
    - [ ] show selected ICE candidate on demo site
    - [ ] split user API and internal setting API
    - [ ] force non-trickle on web
    - [ ] better TURN servers setup for demo site

* Issues (discover during development or team test)
    - [ ] publisher rejoin sometime will cause video stucking on subscriber side
    - [ ] we get some broken audio from time to time
    - [ ] sometime it needs 10 seconds to become connected (network problem?)
    - [ ] Safari can't connect

* Refactor
    - [ ] redesign the room metadata
    - [ ] redesign the SDP flow
