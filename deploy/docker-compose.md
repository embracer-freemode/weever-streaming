# Deploy with Docker Compose

```bash
# start your docker daemon
sudo systemctl start docker
# deploy
# run in the root folder of the project, the one with "docker-compose.yml" in it
docker-compose up
# check status
docker-compose ls
docker-compose ps
docker-compose logs -f sfu1
# remove
docker-compose down
```
