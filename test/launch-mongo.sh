#/bin/bash
podman run --rm \
  --name mongo-test \
  -p 27017:27017 \
  -e MONGO_INITDB_ROOT_USERNAME=testing\
  -e MONGO_INITDB_ROOT_PASSWORD=testing\
  mongo:latest
  # -e MONGODB_INITDB_ROOT_USERNAME=testing\
  # -e MONGODB_INITDB_ROOT_PASSWORD=testing\
  # mongodb/mongodb-atlas-local:latest