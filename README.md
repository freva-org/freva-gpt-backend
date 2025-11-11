# freva-GPT-backend

This github repository is for the backend of the freva-GPT project.

## Running the backend

### Using Pre-built Images
```bash
docker pull ghcr.io/freva-org/freva-gpt2-backend:latest

# Or with CrocoDash: 
docker ghcr.io/freva-org/freva-gpt2-backend-crocodash:latest
```

Configure and run with environment variables:
```bash
# Option 1: Using .env file
cp .env.example .env
# Edit .env with your settings
docker run -p 8502:8502 --env-file .env ghcr.io/freva-org/freva-gpt2-backend:latest

# Option 2: Pass individual variables
docker run -p 8502:8502 \
  -e AUTH_KEY="your-secret-key" \
  -e OPENAI_API_KEY="sk-..." \
  -e INSTANCE_NAME="prod" \
  ghcr.io/freva-org/freva-gpt2-backend:latest
```

**Environment Variables:**
| Variable | Description | Default |
|----------|-------------|---------|
| `AUTH_KEY` | Authentication string that frontend must match (being phased out for OAuth2) | Required |
| `OPENAI_API_KEY` | Your OpenAI API key for accessing OpenAI models | Required |
| `LITE_LLM_ADDRESS` | LiteLLM proxy address | `http://litellm:4000` |
| `INSTANCE_NAME` | Instance identifier for running multiple instances (e.g., "dev", "prod") | Required |
| `HOST` | Bind address | `0.0.0.0` |
| `BACKEND_PORT` | Internal backend port | `8502` |
| `TARGET_PORT` | External accessible port | `8502` |
| `ALLOW_GUESTS` | Allow unauthenticated access (`true`/`false`) | `true` |
| `MONGODB_DATABASE_NAME` | MongoDB database name for thread storage | `chatbot` |
| `MONGODB_COLLECTION_NAME` | MongoDB collection name | `threads` |

### Building from Source

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

## Releasing

1. Update version in `Cargo.toml` and push to main.
2. Run `make release` to create and push tag
3. GitHub Actions automatically builds multi-arch images (amd64/arm64) for both base(without Crocodash) and with-Crocodash