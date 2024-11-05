# freva-GPT2-backend

This gitlab project is for the backend of the second version of the freva-GPT project.

For an example on how to use the API, check out the jupyter notebook `example.ipynb` in the top directory of this repo.

Additionally, the notebook I use for testing the versions (`testing.ipynb`) is also in this repo to document the usage of the API in developement.

## Running the backend

To run the backend, navigate to this folder so that this file is on the top level.
Then run `podman-compose build` to build the project and `podman-compose up -d` to launch it.
This requires `podman-compose` and `podman` to be configured correctly and that no container with the name `freva-gpt2-backend-instance` is running.
In that case, run `podman-compose down` and then to start the containers `podman-compose up -d`.

That means that when the containers are already running an changes are made, executing the following commands in order relaunches the containers with the changes applied:

```bash
podman-compose build # builds the changes. Everything is cached; changing the code should only take 10 seconds, changing pendencies takes up to 7 minutes for conda to install them
podman-compose down # stop and remove the previous containers of freva-gpt-backend and ollama so the new ones can be properly launched in it's place
podman-compose up -d # start the containers in a standardized way
# Optionally
podman logs -fl # follow the logs of the latest container. If it goes to `Starting Server on 0.0.0.0:8502`, it worked. Can also be used to quickly see all warnings and errors the backend emitted.
```

The `docker-compose.yaml` file defines how the containers are deployed, defining settings for images used, port-forwarding, mounted volumes, and network settings. For the `freva-gpt2-backend` it also defines how
to build the image, which is mainly just a way to tell podman to load the Dockerfile at `dockerfiles/chatbot-backend/Dockerfile`. Changes in that file will be reflected in the next build.

### Ollama

The backend supports a local ollama instance to be running.
It is also started when running the `podman-compose up` command.
The models that want to be used need to be pulled from within the container with `podman exec -it ollama /bin/bash`.
For example, if we want to download `Llama3.2` into the container, we would run the following command inside it:
`ollama pull llama3.2`
