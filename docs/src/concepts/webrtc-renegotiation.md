# WebRTC Renegotiation

After WebRTC is connected,
you can still update the SDP for latest media status.

To achieve WebRTC Renegotiation,
you simply go through the WebRTC Negotiation flow one more time.
Send SDP Offer to the other peer. Receive and set SDP Answer.

The SDP Offer can be initiated by any peer,
not necessary the peer who sent SDP Offer in first setup.
