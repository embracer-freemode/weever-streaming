# Join as Subscriber

JavaScript example (based on browser API):

```javascript
// create WebRTC peer connection
let pc = new RTCPeerConnection();
// create a data channel labeled as "control"
let dc = pc.createDataChannel("control");

// setup callback
pc.onicecandidate = function(event) {
    if (event.candidate) {
        // ...
    } else {
        let offer = pc.localDescription;

        let type = "sub";
        let room = "testroom";  // modify this
        let id = "testuser";    // modify this
        let token = "mysecret"; // modify this
        let url = `/${type}/${room}/${id}`;   // modify this

        // send to Weever Streaming
        fetch(url, {
            method: "POST",
            headers: {
                "Content-Type": "application/sdp",
                "Authorization": `Bearer ${token}`,
            },
            body: offer.sdp,
        }).then(res => {
            res.text().then(function(answer) {
                pc.setRemoteDescription(new RTCSessionDescription({"type": "answer", "sdp": answer}));
            })
        });
    }
}

// data channel messages
dc.onmessage = e => {
    // WebRTC Renegotiation
    if (e.data.startsWith("SDP_OFFER ") == true) {
        let offer = e.data.slice(10);
        pc.setRemoteDescription(new RTCSessionDescription({"type": "offer", "sdp": offer}));
        pc.createAnswer()
            .then(answer => {
                pc.setLocalDescription(answer);
                dc.send("SDP_ANSWER " + answer.sdp);
            });
    }
}

pc.createOffer()
    .then(offer => {
        // set local SDP offer
        // this will trigger ICE gathering, and then onicecandidate callback
        pc.setLocalDescription(offer);
    });
```
