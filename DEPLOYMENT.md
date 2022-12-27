# Deployment (Kubernetes)

* SFU can be deployed with StatefulSet.
* Override the SSL certs in Pod.
* A LoadBalancer in front of public web server.
* Setup liveness/readiness probe, point to SFU private web server.
* Setup preStop hook, point to SFU private web server.
* NATS instance can be another service or SFU Pod sidecar.
* Redis instance can be another service.
* HPA based on CPU usage, dynamically grows the Pod count.
* Can use other complex mechanism to control minimum Pod count if you need to schedule big events. For example, a cronjob to read business data and calculate a desired minimum Pod count.
* Use/Setup TURN server if needed.
