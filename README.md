Horizontal Scaling WebRTC SFU
========================================

A WebRTC SFU server aim to be horizontal scalable.

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
* [NATS server](https://nats.io)


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

Go check the ``tarpaulin-report.html``:

![TODO-UPDATE-test-cov1](https://user-images.githubusercontent.com/2716047/128280082-ddf88c95-b1f8-4d2c-8a52-2cf2ad2c6b74.png)
![TODO-UPDATE-test-cov2](https://user-images.githubusercontent.com/2716047/128280084-46101db4-615b-4133-9289-73ba0b7060ef.png)


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

Docs site
------------------------------

You can build API documentation site via command:

```sh
cargo doc
```


Launching Flow in Program
------------------------------


States save/load
------------------------------



Extra Learning
========================================

This project shows that:

* we can leverage existing WebRTC libraries to build our SFU server
* we can reuse whatever pub/sub pattern message bus for media distribution
* the WebRTC signaling exchange can simplify to 1 HTTP request/response



Future Works
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
