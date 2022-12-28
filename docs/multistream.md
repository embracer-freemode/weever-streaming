# Multistream

A subscriber connection can receive multiple streams from multiple publishers.

All the publishers' streams will be inclued in one SDP for subscriber.

The stream id in SDP will be the publisher id.
So subscriber can identify each stream after parsing the SDP.

When a publisher join or leave,
server will trigger WebRTC renegociation for subscriber,
new SDP will be sent via data channel.
Subscriber can use new SDP to get latest streams status in the room.
