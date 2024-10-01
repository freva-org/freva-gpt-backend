#!/bin/bash
podman run -d --device nvidia.com/gpu=all -v ollama:/home/b/b380001/ollama-podman/.ollama --network freva-gpt --name ollama ollama/ollama
