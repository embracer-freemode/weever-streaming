# Data Channel API

A data channel labeled as "**control**" will be setup from server.
This data channel will be used for WebRTC Renegotiation or other connection actions.


## Publisher

| Command    | Server/Client Send | Usage                                                                                                      |
| ---------- | ------------------ | ---------------------------------------------------------------------------------------------------------- |
| SDP_OFFER  | Client Send        | WebRTC Renegotiation                                                                                       |
| SDP_ANSWER | Server Send        | WebRTC Renegotiation                                                                                       |
| STOP       | Client Send        | Actively close peer connection. (There is connection timeout for leaving peers even without this command.) |
| OK         | Server Send        | Let client know server received the message but there is no special actions.                               |


## Subscriber

| Command    | Server/Client Send | Usage                                                                                                      |
| ---------- | ------------------ | ---------------------------------------------------------------------------------------------------------- |
| SDP_OFFER  | Server Send        | WebRTC Renegotiation                                                                                       |
| SDP_ANSWER | Client Send        | WebRTC Renegotiation                                                                                       |
| PUB_JOIN   | Server Send        | A publisher joined the room                                                                                |
| PUB_LEFT   | Server Send        | A publisher left the room                                                                                  |
| STOP       | Client Send        | Actively close peer connection. (There is connection timeout for leaving peers even without this command.) |
| OK         | Server Send        | Let client know server received the message but there is no special actions.                               |
