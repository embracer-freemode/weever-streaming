# Scaling

Weever Streaming scaling can be done by adding more instances.


Possible setup:

* All Weever Streaming shared the same external NATS service. In this style, Weever Streaming scaling doesn't bind with NATS scaling.
* Each Weever Streaming has its own NATS sidecar. And NATS sidecars form a cluster by themselves. In this style, Weever Streaming scaling bind with NATS scaling.
