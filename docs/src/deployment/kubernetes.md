# Kubernetes

(TODO: add helm chart in)

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
* Authentication can be enabled via `--auth`. It's using Bearer Token mechanism. Token can be set via private web server. Data will be saved in Redis (1 day TTL).
* Management can be done via another service that talks to SFU via private web server.
* CORS can be set via `--cors-domain <DOMAIN>`
* Most of configs can be set via both CLI arguments and environment variables.
