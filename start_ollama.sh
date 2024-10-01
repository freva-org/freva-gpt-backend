#!/bin/bash
podman run -d --device nvidia.com/gpu=all -v ollama:/home/b/b380001/ollama-podman/.ollama -p 11434:11434 --name ollama ollama/ollama
