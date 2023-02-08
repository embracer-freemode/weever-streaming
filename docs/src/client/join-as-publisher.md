# Join as Publisher

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

        let type = "pub";
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

// grab media (audio/video)
navigator.mediaDevices.getUserMedia({ audio: true, video: true })
    .then(stream => {
        stream.getTracks().forEach(track => pc.addTrack(track, stream));
        pc.createOffer()
            .then(offer => {
                // set local SDP offer
                // this will trigger ICE gathering, and then onicecandidate callback
                pc.setLocalDescription(offer);
            });
    });
```
