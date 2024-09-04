#!/bin/bash
podman build --cgroup-manager=cgroupfs -t freva-gpt2-backend -f dockerfiles/chatbot-backend/Dockerfile .
