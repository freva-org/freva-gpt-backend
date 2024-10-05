# freva-GPT2-backend

This gitlab project is for the backend of the second version of the freva-GPT project.

For an example on how to use the API, check out the jupyter notebook `example.ipynb` in the top directory of this repo.

Additionally, the notebook I use for testing the versions (`testing.ipynb`) is also in this repo to document the usage of the API in developement.

## Running the backend

To run the backend, navigate to this folder so that this file is on the top level.
Then run `podman-build.sh` to build the project and `compose.sh` to launch it.
This requires podman to be configured correctly and that no container with the name `freva-gpt2-backend-instance` is running.
In that case, stop and remove that container and re-run `compose.sh`.

That means that when the container is already running an changes are made, executing the following commands in order relaunches the container with the changes applied:

```bash
./podman-build.sh # builds the changes. Everything is cached; changing the code should only take 10 seconds, changing pendencies takes up to 7 minutes for conda to install them
./stop_backend # stop and remove the previous container of freva-gpt-backend so the new one can be properly launched in it's place
./compose.sh # start the container in a standardized way
# Optionally
podman logs -fl # follow the logs of the latest container. If it goes to `Starting Server on 0.0.0.0:8502`, it worked. Can also be used to quickly see all warnings and errors the backend emitted.
```

The `podman-build.sh` script is mainly just a way to tell podman to load the Dockerfile at `dockerfiles/chatbot-backend/Dockerfile`, so changes in that file will be reflected in the next build.

### Ollama

The backend supports a local ollama instance to be running.
After pulling the official ollama docker container with podman, it can be started with the `./start_ollama.sh` script.
The models that want to be used need to be pulled from within the container with `podman exec -it ollama /bin/bash`.
