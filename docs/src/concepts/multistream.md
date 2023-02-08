# Multistream

Multistream means putting arbitrary amount of audio/video tracks in the same PeerConnection. \
(It's done via [Unified Plan](https://datatracker.ietf.org/doc/html/draft-roach-mmusic-unified-plan-00) of SDP)

Combine with the widely adopted RTP BUNDLE and RTCP multiplexing.
This means we can serve both audio and video at the same time,
plus the control protocol,
for any amount of peers' media all in "one single socket connection".


In Weever Streaming,
the [subscribers](./publisher-subscriber.md#subscriber) will leverage multistream
for receiving all audio/video (from multiple [publishers](./publisher-subscriber.md#publisher)) in single connection.

By Weever Streaming convention,
the stream id in SDP will be the publisher id.
So subscriber can identify each stream after parsing the SDP.
This also means subscribers can use info in SDP to know all the publishers.

To keep it simple,
publishers in Weever Streaming still use 1 connection for each outgoing streams.
Only subscribers fully leverage the multistream.


When a publisher join or leave,
server will trigger [WebRTC Renegotiation](./webrtc-renegotiation.md) for subscribers.
New SDP will be sent via Data Channel.
Subscribers can use new SDP to get latest streams status in the room.


For more information about multistream, you can checkout Janus's article [Multistream is here!](https://www.meetecho.com/blog/multistream/).
