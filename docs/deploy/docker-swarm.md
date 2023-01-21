# Deploy with Docker Swarm

## Start up docker swarm mode

```bash
# start your docker daemon
sudo systemctl start docker
# run docker swarm node
docker swarm init
```

## Have all the nodes you want join the swarm

```bash
# (optional) join the cluster in another machine
docker swarm join --token <token> <ip:port>
```

## Deploy Weever-streaming up

```bash
# run in the root folder of the project, the one with "docker-compose.yml" in it
docker stack deploy --compose-file docker-compose.yml weever-streaming

# check status
docker stack ls
docker stack services weever-streaming
docker stack ps weever-streaming
docker service logs -f weever-streaming_sfu1
```

## remove Weever-streaming from the swarm

```bash
docker stack rm weever-streaming
```
