# Deploy with Docker Swarm

```bash
# start your docker daemon
sudo systemctl start docker
# run docker swarm node
docker swarm init
# (optional) join the cluster in another machine
docker swarm join --token <token> <ip:port>
# deploy
# run in the root folder of the project, the one with "docker-compose.yml" in it
docker stack deploy --compose-file docker-compose.yml weever-streaming
# check status
docker stack ls
docker stack services weever-streaming
docker stack ps weever-streaming
docker service logs -f weever-streaming_sfu1
# remove
docker stack rm weever-streaming
```
