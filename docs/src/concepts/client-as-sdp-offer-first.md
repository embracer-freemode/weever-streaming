# Client as SDP Offer first

In Weever Streaming, the clients always will be the SDP Offer side for connection setup.
No matter the clients want to be publisher or subscriber.
This make clients WebRTC connection setup to be 1 HTTP POST and response.

After WebRTC is connected, Weever Streaming will use Data Channel for following WebRTC Renegotiation.
