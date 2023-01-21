# Deploy with Docker Compose

## start your docker daemon

```bash
# start your docker daemon
sudo systemctl start docker
```

## bring up weever-streaming

```bash
# run in the root folder of the project, the one with "docker-compose.yml" in it
docker-compose up
# check status
docker-compose ls
docker-compose ps
docker-compose logs -f sfu1
```

## remove weever-streaming

```bash
docker-compose down
```
