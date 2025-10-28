# freva-GPT-backend

This github repository is for the backend of the freva-GPT project at the DKRZ.

## Running the backend

To run the backend, first install `podman` (or `docker`, but podman is preferred).

Check the `.env.example` file to configure the environment, preferably using a `.env` file.
Note that missing config will be warned against when the backend is started.

Run `podman-compose build` to build the project and `podman-compose up -d` to launch it.

```bash
podman-compose build # builds the changes. Note that everything is cached, so
podman-compose down # stop and remove the previous containers
podman-compose up -d # start the containers without binding this terminal session to it
```

## Containers

All required containers are defined in the `docker-compose.yml` file; there are currently three.
The `freva-gpt2-backend` container is the main one, containing the main logic.
`litellm` (see [here](https://www.litellm.ai/); [repo](https://github.com/BerriAI/litellm)) is a multi-purpose, community driven python library and proxy server.
Its proxy server is used here to unify the protocol to communicate with different models and model providers.
The `ollama` container (see [here](https://ollama.com/); [repo](https://github.com/ollama/ollama)) is used to locally run models.

## Configuration

Besides the main configuration via the environment variables in the `.env` file, there is also the `litellm_config.yml` file which describes which LLMs are accessable where (follows [the litellm gateway config](https://docs.litellm.ai/docs/proxy/configs)).
Note that the very first in the file is the default LLM to use if not specified by the user.
