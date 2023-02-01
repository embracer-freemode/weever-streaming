# Deploy with Docker Compose

```
this will launch: 1 Redis, 3 NATS, 3 WebRTC SFU (Weever Streaming)

  ┌──────┐    ┌──────┐    ┌──────┐
  │ SFU1 ├──┐ │ SFU2 ├──┐ │ SFU3 ├──┐
  └───┬──┘  │ └───┬──┘  │ └───┬──┘  │
      │     │     │     │     │     │
  ┌───▼───┐ │ ┌───▼───┐ │ ┌───▼───┐ │
  │ NATS1 ◄─┼─► NATS2 ◄─┼─► NATS3 │ │
  └───────┘ │ └───────┘ │ └───────┘ │
            │           │           │
  ┌─────────▼───────────▼───────────▼──┐
  │               Redis                │
  └────────────────────────────────────┘
```


## Start your docker daemon

```bash
# start your docker daemon
sudo systemctl start docker
```

## Bring up Weever Streaming

```bash
# run in the root folder of the project, the one with "docker-compose.yml" in it
docker-compose up
# check status
docker-compose ls
docker-compose ps
docker-compose logs -f sfu1
```

## Remove Weever Streaming

```bash
docker-compose down
```
